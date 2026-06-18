//! JSON serialization of the status views (`--json`, ADR-0006). Built from the
//! same `render::Row`s the table renders, so the two outputs never drift. Shape:
//! `{ "remotes": [...], "repos": [ { repo, lifecycle, others?, branches: [...] } ] }`.

use crate::render::Row;
use git_redundancy_core::{BranchSync, WorkingTree};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Fleet<'a> {
    pub remotes: &'a [String],
    pub repos: Vec<RepoJson>,
}

#[derive(Serialize)]
pub struct RepoJson {
    pub repo: String,
    pub lifecycle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub others: Option<u32>,
    pub branches: Vec<BranchJson>,
}

#[derive(Serialize)]
pub struct BranchJson {
    pub branch: String,
    pub current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_tree: Option<WtJson>,
    pub remotes: BTreeMap<String, SyncJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Serialize)]
pub struct WtJson {
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicts: u32,
}

#[derive(Serialize)]
pub struct SyncJson {
    pub state: &'static str,
    pub ahead: u32,
    pub behind: u32,
    pub conflict: bool,
}

fn wt_json(wt: &WorkingTree) -> WtJson {
    WtJson {
        staged: wt.staged,
        unstaged: wt.unstaged,
        untracked: wt.untracked,
        conflicts: wt.conflicts,
    }
}

fn sync_json(s: &BranchSync) -> SyncJson {
    let (state, ahead, behind, conflict) = match s {
        BranchSync::NoRemoteBranch => ("new", 0, 0, false),
        BranchSync::UpToDate => ("up-to-date", 0, 0, false),
        BranchSync::Ahead(n) => ("ahead", *n, 0, false),
        BranchSync::Behind(n) => ("behind", 0, *n, false),
        BranchSync::Diverged {
            ahead,
            behind,
            conflict,
        } => (
            if *conflict { "conflict" } else { "diverged" },
            *ahead,
            *behind,
            *conflict,
        ),
    };
    SyncJson {
        state,
        ahead,
        behind,
        conflict,
    }
}

fn branch_json(remotes: &[String], row: &Row) -> BranchJson {
    let mut map = BTreeMap::new();
    for (remote, cell) in remotes.iter().zip(row.remote_cells.iter()) {
        if let Some(s) = cell {
            map.insert(remote.clone(), sync_json(s));
        }
    }
    BranchJson {
        branch: row.branch.clone(),
        current: row.is_current,
        working_tree: row.wt.as_ref().map(wt_json),
        remotes: map,
        action: row.action.clone(),
    }
}

/// Group flat fleet rows back into per-repo JSON (a row with a non-empty `repo`
/// starts a new repo; `(home)` placeholder rows carry no branch).
pub fn fleet<'a>(remotes: &'a [String], rows: &[Row]) -> Fleet<'a> {
    let mut repos: Vec<RepoJson> = Vec::new();
    for row in rows {
        if !row.repo.is_empty() {
            repos.push(RepoJson {
                repo: row.repo.clone(),
                lifecycle: row.lifecycle.clone(),
                others: None,
                branches: Vec::new(),
            });
        }
        let Some(cur) = repos.last_mut() else {
            continue;
        };
        if row.others.is_some() {
            cur.others = row.others;
        }
        if row.branch != "(home)" {
            cur.branches.push(branch_json(remotes, row));
        }
    }
    Fleet { remotes, repos }
}

/// Detail (one repo): that repo's branches with their `sync` actions.
pub fn detail(repo: &str, lifecycle: &str, remotes: &[String], rows: &[Row]) -> RepoJson {
    RepoJson {
        repo: repo.to_string(),
        lifecycle: lifecycle.to_string(),
        others: None,
        branches: rows.iter().map(|r| branch_json(remotes, r)).collect(),
    }
}
