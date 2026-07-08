//! Parse `git status --porcelain=v2 -z` into [`WorkingTree`] counts.
//!
//! Pure string→counts; this is a property-test target. The porcelain v2 format
//! is one record per NUL-separated token:
//! - `1 <XY> …`  ordinary changed entry
//! - `2 <XY> …`  rename/copy entry — followed by a *separate* token: the original path
//! - `u <XY> …`  unmerged (conflict) entry
//! - `? <path>`  untracked
//! - `! <path>`  ignored
//! - `# …`       header lines (branch info)
//!
//! `X` is the index/staged status, `Y` the worktree/unstaged status; `.` means
//! "unchanged on that side".

use crate::model::WorkingTree;

/// Parse the raw output of `git status --porcelain=v2 -z`.
pub fn parse_porcelain_v2_z(input: &str) -> WorkingTree {
    let mut wt = WorkingTree::default();
    let mut tokens = input.split('\0');

    while let Some(tok) = tokens.next() {
        let Some(&kind) = tok.as_bytes().first() else {
            continue; // empty token (e.g. trailing NUL)
        };
        match kind {
            b'1' | b'2' => {
                if let Some(xy) = tok.split(' ').nth(1) {
                    let mut chars = xy.chars();
                    let x = chars.next().unwrap_or('.');
                    let y = chars.next().unwrap_or('.');
                    if x != '.' {
                        wt.staged = wt.staged.saturating_add(1);
                    }
                    if y != '.' {
                        wt.unstaged = wt.unstaged.saturating_add(1);
                    }
                }
                if kind == b'2' {
                    // Rename/copy records carry the original path as the next token.
                    let _ = tokens.next();
                }
            }
            b'u' => wt.conflicts = wt.conflicts.saturating_add(1),
            b'?' => wt.untracked = wt.untracked.saturating_add(1),
            _ => {} // '!' ignored, '#' headers, anything else
        }
    }
    wt
}

/// What a changed path needs before it can be included in a commit (the
/// `gr` default-command review loop, ADR-0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Not tracked by git at all (`?` record).
    Untracked,
    /// Has an unstaged worktree delta (`Y != '.'`) — staging it needs a `git add`.
    Modified,
    /// Already fully staged (`X != '.'`, `Y == '.'`) — nothing left to add.
    StagedOnly,
    /// Unmerged (`u` record) — never staged automatically; surfaced, not touched.
    Conflict,
}

/// One changed path from `git status --porcelain=v2 -z`, classified for the
/// review loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    pub kind: EntryKind,
}

/// Parse `git status --porcelain=v2 -z` into per-path entries (companion to
/// [`parse_porcelain_v2_z`], which only aggregates counts). Same token walk;
/// header (`#`) and ignored (`!`) records are skipped, same as the counter.
pub fn parse_status_entries_v2_z(input: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut tokens = input.split('\0');

    while let Some(tok) = tokens.next() {
        let Some(&kind) = tok.as_bytes().first() else {
            continue; // empty token (e.g. trailing NUL)
        };
        match kind {
            b'1' | b'2' => {
                let xy = tok.split(' ').nth(1).unwrap_or("..");
                let mut chars = xy.chars();
                let x = chars.next().unwrap_or('.');
                let y = chars.next().unwrap_or('.');
                // Fixed field count before the path differs between an ordinary
                // ("1") and a rename/copy ("2") record; splitn's last part is
                // everything after those fields, so an embedded space in the
                // path itself is preserved rather than truncated.
                let field_count = if kind == b'1' { 9 } else { 10 };
                if let Some(path) = tok.splitn(field_count, ' ').last() {
                    let entry_kind = if y != '.' {
                        EntryKind::Modified
                    } else if x != '.' {
                        EntryKind::StagedOnly
                    } else {
                        continue; // "no change on either side" shouldn't occur
                    };
                    entries.push(StatusEntry {
                        path: path.to_string(),
                        kind: entry_kind,
                    });
                }
                if kind == b'2' {
                    let _ = tokens.next(); // consume the rename's original-path token
                }
            }
            b'u' => {
                if let Some(path) = tok.splitn(11, ' ').last() {
                    entries.push(StatusEntry {
                        path: path.to_string(),
                        kind: EntryKind::Conflict,
                    });
                }
            }
            b'?' => {
                if let Some(path) = tok.splitn(2, ' ').last() {
                    entries.push(StatusEntry {
                        path: path.to_string(),
                        kind: EntryKind::Untracked,
                    });
                }
            }
            _ => {} // '!' ignored, '#' headers, anything else
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_clean() {
        assert!(parse_porcelain_v2_z("").is_clean());
        assert!(parse_porcelain_v2_z("\0").is_clean());
    }

    #[test]
    fn counts_a_realistic_sample() {
        // header, one staged+modified, one modified-only, one untracked, one rename, one conflict
        let sample = concat!(
            "# branch.oid abc123\0",
            "# branch.head main\0",
            "1 M. N... 100644 100644 100644 aaa bbb staged.rs\0",
            "1 .M N... 100644 100644 100644 ccc ddd worktree.rs\0",
            "? new_file.rs\0",
            "2 R. N... 100644 100644 100644 eee fff R100 newname.rs\0",
            "old_name.rs\0",
            "u UU N... 100644 100644 100644 100644 ggg hhh iii conflict.rs\0",
        );
        let wt = parse_porcelain_v2_z(sample);
        // staged: staged.rs (M.) + rename (R.) = 2
        assert_eq!(wt.staged, 2, "staged");
        // unstaged: worktree.rs (.M) = 1
        assert_eq!(wt.unstaged, 1, "unstaged");
        assert_eq!(wt.untracked, 1, "untracked");
        assert_eq!(wt.conflicts, 1, "conflicts");
        assert!(!wt.is_clean());
    }

    #[test]
    fn rename_original_path_token_is_not_miscounted() {
        // The "old_name.rs" token must be consumed by the rename record, not
        // treated as its own entry.
        let sample = "2 R. N... 1 1 1 a b R100 new.rs\0old.rs\0";
        let wt = parse_porcelain_v2_z(sample);
        assert_eq!(wt.staged, 1);
        assert_eq!(wt.unstaged, 0);
        assert_eq!(wt.untracked, 0);
    }

    proptest::proptest! {
        /// The parser is total: never panics on arbitrary bytes-as-text.
        #[test]
        fn never_panics(s in ".*") {
            let _ = parse_porcelain_v2_z(&s);
        }

        /// Pure untracked lines count exactly.
        #[test]
        fn untracked_count_matches(n in 0usize..200) {
            let input: String = (0..n).map(|i| format!("? f{i}.rs\0")).collect();
            let wt = parse_porcelain_v2_z(&input);
            proptest::prop_assert_eq!(wt.untracked as usize, n);
            proptest::prop_assert_eq!(wt.staged, 0);
        }
    }

    #[test]
    fn entries_classify_a_realistic_sample() {
        // Same sample as `counts_a_realistic_sample`, checked per-path this time.
        let sample = concat!(
            "# branch.oid abc123\0",
            "# branch.head main\0",
            "1 M. N... 100644 100644 100644 aaa bbb staged.rs\0",
            "1 .M N... 100644 100644 100644 ccc ddd worktree.rs\0",
            "? new_file.rs\0",
            "2 R. N... 100644 100644 100644 eee fff R100 newname.rs\0",
            "old_name.rs\0",
            "u UU N... 100644 100644 100644 100644 ggg hhh iii conflict.rs\0",
        );
        let entries = parse_status_entries_v2_z(sample);
        assert_eq!(
            entries,
            vec![
                StatusEntry {
                    path: "staged.rs".into(),
                    kind: EntryKind::StagedOnly,
                },
                StatusEntry {
                    path: "worktree.rs".into(),
                    kind: EntryKind::Modified,
                },
                StatusEntry {
                    path: "new_file.rs".into(),
                    kind: EntryKind::Untracked,
                },
                StatusEntry {
                    path: "newname.rs".into(),
                    kind: EntryKind::StagedOnly,
                },
                StatusEntry {
                    path: "conflict.rs".into(),
                    kind: EntryKind::Conflict,
                },
            ]
        );
    }

    #[test]
    fn entries_rename_original_path_token_is_not_a_spurious_entry() {
        let sample = "2 R. N... 1 1 1 a b R100 new.rs\0old.rs\0";
        let entries = parse_status_entries_v2_z(sample);
        assert_eq!(
            entries,
            vec![StatusEntry {
                path: "new.rs".into(),
                kind: EntryKind::StagedOnly,
            }]
        );
    }

    #[test]
    fn entries_path_with_embedded_space_is_preserved() {
        let sample = "1 .M N... 100644 100644 100644 aaa bbb my file.rs\0";
        let entries = parse_status_entries_v2_z(sample);
        assert_eq!(entries[0].path, "my file.rs");
    }

    proptest::proptest! {
        /// The entry parser is also total: never panics on arbitrary bytes-as-text.
        #[test]
        fn entries_never_panics(s in ".*") {
            let _ = parse_status_entries_v2_z(&s);
        }

        /// Every untracked line becomes exactly one `Untracked` entry with its path.
        #[test]
        fn entries_untracked_count_matches(n in 0usize..200) {
            let input: String = (0..n).map(|i| format!("? f{i}.rs\0")).collect();
            let entries = parse_status_entries_v2_z(&input);
            proptest::prop_assert_eq!(entries.len(), n);
            proptest::prop_assert!(entries.iter().all(|e| e.kind == EntryKind::Untracked));
        }
    }
}
