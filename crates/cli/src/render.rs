//! Render the status table. Pure string assembly over already-collected rows.
//! Colors are emitted as ANSI escapes only when `color` is true; tabled's `ansi`
//! feature measures display width correctly so colored cells still align.

use git_redundancy_core::{BranchSync, WorkingTree};
use tabled::builder::Builder;
use tabled::settings::Style;

/// One line of the status table.
pub struct Row {
    pub repo: String,
    pub branch: String,
    pub is_current: bool,
    /// Working-tree counts, shown only on the repo's current-branch row.
    pub wt: Option<WorkingTree>,
    /// Per-shown-remote cell, aligned with the `remotes` header order.
    /// `None` = the repo doesn't have that remote.
    pub remote_cells: Vec<Option<BranchSync>>,
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
        None => (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
        Some(w) => (
            count(w.staged, "32", color),    // green  — ready to commit
            count(w.unstaged, "33", color),  // yellow — modified, not staged
            count(w.untracked, "36", color), // cyan   — untracked
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

/// Build the table as a printable string.
pub fn table(remotes: &[String], rows: &[Row], color: bool) -> String {
    let mut builder = Builder::default();

    let mut header = vec![
        "Repo".to_string(),
        "Branch".to_string(),
        "S".to_string(),
        "U".to_string(),
        "?".to_string(),
        "Cf".to_string(),
    ];
    header.extend(remotes.iter().cloned());
    builder.push_record(header);

    for row in rows {
        let (s, u, q, c) = wt_cells(row.wt, color);
        let branch = if row.is_current {
            format!("* {}", row.branch)
        } else {
            format!("  {}", row.branch)
        };
        let mut record = vec![row.repo.clone(), branch, s, u, q, c];
        record.extend(row.remote_cells.iter().map(|s| remote_cell(s, color)));
        builder.push_record(record);
    }

    let mut table = builder.build();
    table.with(Style::rounded());
    table.to_string()
}
