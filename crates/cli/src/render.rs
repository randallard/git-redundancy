//! Render the status table. Pure string assembly over already-collected rows.

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

fn wt_cell(wt: Option<WorkingTree>) -> (String, String, String, String) {
    match wt {
        None => (String::new(), String::new(), String::new(), String::new()),
        Some(w) => {
            let f = |n: u32| {
                if n == 0 {
                    "·".to_string()
                } else {
                    n.to_string()
                }
            };
            (f(w.staged), f(w.unstaged), f(w.untracked), f(w.conflicts))
        }
    }
}

fn remote_cell(sync: &Option<BranchSync>) -> String {
    match sync {
        None => "-".to_string(),
        Some(BranchSync::NoRemoteBranch) => "new".to_string(),
        Some(BranchSync::UpToDate) => "ok".to_string(),
        Some(BranchSync::Ahead(n)) => format!("↑{n}"),
        Some(BranchSync::Behind(n)) => format!("↓{n}"),
        Some(BranchSync::Diverged {
            ahead,
            behind,
            conflict,
        }) => {
            let tag = if *conflict { "CONFLICT" } else { "diverged" };
            format!("↑{ahead}↓{behind} {tag}")
        }
    }
}

/// Build the table as a printable string.
pub fn table(remotes: &[String], rows: &[Row]) -> String {
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
        let (s, u, q, c) = wt_cell(row.wt);
        let branch = if row.is_current {
            format!("* {}", row.branch)
        } else {
            format!("  {}", row.branch)
        };
        let mut record = vec![row.repo.clone(), branch, s, u, q, c];
        record.extend(row.remote_cells.iter().map(remote_cell));
        builder.push_record(record);
    }

    let mut table = builder.build();
    table.with(Style::rounded());
    table.to_string()
}
