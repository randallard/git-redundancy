//! Render the status tables. Pure string assembly over already-collected rows.
//! Colors are emitted as ANSI escapes only when `color` is true; tabled's `ansi`
//! feature measures display width correctly so colored cells still align.

use git_redundancy_core::{BranchSync, WorkingTree};
use tabled::builder::Builder;
use tabled::settings::Style;

/// One line of a status table.
pub struct Row {
    pub repo: String,
    pub branch: String,
    pub is_current: bool,
    /// Working-tree counts, shown only on the repo's current-branch row.
    pub wt: Option<WorkingTree>,
    /// Per-shown-remote cell, aligned with the `remotes` header order.
    /// `None` = the repo doesn't have that remote.
    pub remote_cells: Vec<Option<BranchSync>>,
    /// Lifecycle label (`linked`/`local-only`/`home-only`/`?`), fleet view only,
    /// shown on the repo's first row. Empty = don't render.
    pub lifecycle: String,
    /// Backup-server presence (`ok`/`miss`/`?`), fleet view only, shown on the
    /// repo's first row when a `[backup]` server is configured. Empty = no cell.
    pub backup: String,
    /// "Others need attention" count (fleet, current-branch view): branches
    /// besides the shown one that aren't up-to-date. `None`/0 = blank.
    pub others: Option<u32>,
    /// What `sync` would do for this branch (detail view only).
    pub action: Option<String>,
}

impl Row {
    /// A blank row with no lifecycle/others/action — the common case builders
    /// fill in selectively.
    pub fn new(repo: String, branch: String, is_current: bool) -> Self {
        Row {
            repo,
            branch,
            is_current,
            wt: None,
            remote_cells: Vec::new(),
            lifecycle: String::new(),
            backup: String::new(),
            others: None,
            action: None,
        }
    }
}

/// Wrap `s` in an ANSI SGR sequence when `color` is on, otherwise return it plain.
fn paint(s: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// A working-tree count cell: dim `·` for zero, otherwise colored count.
fn count(n: u32, code: &str, color: bool) -> String {
    if n == 0 {
        paint("·", "2", color)
    } else {
        paint(&n.to_string(), code, color)
    }
}

fn wt_cells(wt: Option<WorkingTree>, color: bool) -> (String, String, String, String) {
    match wt {
        None => (String::new(), String::new(), String::new(), String::new()),
        Some(w) => (
            count(w.staged, "32", color),      // green  — ready to commit
            count(w.unstaged, "33", color),    // yellow — modified, not staged
            count(w.untracked, "36", color),   // cyan   — untracked
            count(w.conflicts, "1;31", color), // bold red — unmerged
        ),
    }
}

fn remote_cell(sync: &Option<BranchSync>, color: bool) -> String {
    match sync {
        None => paint("-", "2", color),
        Some(BranchSync::NoRemoteBranch) => paint("new", "36", color),
        Some(BranchSync::UpToDate) => paint("ok", "2", color),
        Some(BranchSync::Ahead(n)) => paint(&format!("↑{n}"), "32", color),
        Some(BranchSync::Behind(n)) => paint(&format!("↓{n}"), "33", color),
        Some(BranchSync::Diverged {
            ahead,
            behind,
            conflict,
        }) => {
            let tag = if *conflict { "CONFLICT" } else { "diverged" };
            let code = if *conflict { "1;31" } else { "33" };
            paint(&format!("↑{ahead}↓{behind} {tag}"), code, color)
        }
    }
}

/// Color the lifecycle label by how much attention it wants.
fn lifecycle_cell(label: &str, color: bool) -> String {
    let code = match label {
        "linked" => "2",      // dim — both sides present
        "local-only" => "33", // yellow — needs `create`
        "home-only" => "36",  // cyan — needs `clone`
        _ => "2",             // "?" / unknown
    };
    paint(label, code, color)
}

/// Color the backup-presence cell: `ok` dim-green (mirrored), `miss` red (a gap
/// in redundancy), `?` dim (backup unreachable).
fn backup_cell(label: &str, color: bool) -> String {
    let code = match label {
        "ok" => "32",   // green — present on the backup too
        "miss" => "31", // red — NOT on the backup (redundancy gap)
        _ => "2",       // "?" / unknown — dim
    };
    paint(label, code, color)
}

/// The `+N⚠` "others need attention" cell (blank when nothing else outstanding).
fn others_cell(others: Option<u32>, color: bool) -> String {
    match others {
        Some(n) if n > 0 => paint(&format!("+{n}⚠"), "33", color),
        _ => String::new(),
    }
}

fn branch_label(row: &Row) -> String {
    if row.is_current {
        format!("* {}", row.branch)
    } else {
        format!("  {}", row.branch)
    }
}

/// The fleet table: one block per repo with a lifecycle cell and a `+N⚠` hint.
/// `show_backup` adds a `Bkp` column (only when a `[backup]` server is configured).
pub fn table(remotes: &[String], rows: &[Row], color: bool, show_backup: bool) -> String {
    let mut builder = Builder::default();

    let mut header = vec!["Repo".to_string(), "Life".to_string()];
    if show_backup {
        header.push("Bkp".to_string());
    }
    header.extend([
        "Branch".to_string(),
        "S".to_string(),
        "U".to_string(),
        "?".to_string(),
        "Cf".to_string(),
    ]);
    header.extend(remotes.iter().cloned());
    header.push("⚠".to_string());
    builder.push_record(header);

    for row in rows {
        let (s, u, q, c) = wt_cells(row.wt, color);
        let life = if row.lifecycle.is_empty() {
            String::new()
        } else {
            lifecycle_cell(&row.lifecycle, color)
        };
        let mut record = vec![row.repo.clone(), life];
        if show_backup {
            let bkp = if row.backup.is_empty() {
                String::new()
            } else {
                backup_cell(&row.backup, color)
            };
            record.push(bkp);
        }
        record.extend([branch_label(row), s, u, q, c]);
        record.extend(row.remote_cells.iter().map(|s| remote_cell(s, color)));
        record.push(others_cell(row.others, color));
        builder.push_record(record);
    }

    let mut table = builder.build();
    table.with(Style::rounded());
    table.to_string()
}

/// The detail table (`gr status <repo>`): every branch of one repo with the
/// action `sync` would take.
pub fn detail_table(remotes: &[String], rows: &[Row], color: bool) -> String {
    let mut builder = Builder::default();

    let mut header = vec![
        "Branch".to_string(),
        "S".to_string(),
        "U".to_string(),
        "?".to_string(),
        "Cf".to_string(),
    ];
    header.extend(remotes.iter().cloned());
    header.push("sync".to_string());
    builder.push_record(header);

    for row in rows {
        let (s, u, q, c) = wt_cells(row.wt, color);
        let mut record = vec![branch_label(row), s, u, q, c];
        record.extend(row.remote_cells.iter().map(|s| remote_cell(s, color)));
        record.push(action_cell(row.action.as_deref(), color));
        builder.push_record(record);
    }

    let mut table = builder.build();
    table.with(Style::rounded());
    table.to_string()
}

/// Color the sync-action label by direction/severity.
fn action_cell(action: Option<&str>, color: bool) -> String {
    let Some(a) = action else {
        return String::new();
    };
    let code = if a.starts_with("push") {
        "32" // green — backs up
    } else if a.starts_with("ff") || a == "clone" {
        "36" // cyan — pulls/fetches
    } else if a.contains("CONFLICT") {
        "1;31"
    } else if a == "ok" {
        "2"
    } else {
        "33" // diverged / dirty-blocked
    };
    paint(a, code, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet_row(repo: &str, backup: &str) -> Row {
        let mut r = Row::new(repo.to_string(), "main".to_string(), true);
        r.lifecycle = "linked".to_string();
        r.backup = backup.to_string();
        r
    }

    #[test]
    fn backup_column_only_when_requested() {
        let rows = vec![fleet_row("a", "ok"), fleet_row("b", "miss")];

        // show_backup = true → a `Bkp` header and the per-repo labels appear.
        let with = table(&[], &rows, false, true);
        assert!(with.contains("Bkp"), "expected Bkp header:\n{with}");
        assert!(with.contains("ok") && with.contains("miss"));

        // show_backup = false → no column even though rows carry a backup label.
        let without = table(&[], &rows, false, false);
        assert!(!without.contains("Bkp"));
        assert!(!without.contains("miss"));
    }

    #[test]
    fn backup_cell_unreachable_is_question_mark() {
        let rows = vec![fleet_row("a", "?")];
        let out = table(&[], &rows, false, true);
        assert!(out.contains('?'));
    }
}
