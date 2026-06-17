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
}
