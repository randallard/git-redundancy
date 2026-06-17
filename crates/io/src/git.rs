//! Local reads via system `git` (ADR-0003). No shell, no network: these only
//! read already-present refs and the working tree. Fetch/push land later.

use anyhow::{Context, Result};
use git_redundancy_core::{parse_porcelain_v2_z, AheadBehind, BranchSync, WorkingTree};
use std::path::Path;
use std::process::{Command, Output};

/// Run `git -C <repo> <args…>` with no shell interpretation.
fn git(repo: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("running `git {}` in {}", args.join(" "), repo.display()))
}

fn stdout_string(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Current branch name, or `None` when HEAD is detached.
pub fn current_branch(repo: &Path) -> Result<Option<String>> {
    let out = git(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    if out.status.success() {
        Ok(Some(stdout_string(&out).trim().to_string()))
    } else {
        Ok(None)
    }
}

/// Local branch names (refs/heads), sorted by git's default ordering.
pub fn local_branches(repo: &Path) -> Result<Vec<String>> {
    let out = git(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?;
    Ok(stdout_string(&out)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Configured remotes for the repo.
pub fn remotes(repo: &Path) -> Result<Vec<String>> {
    let out = git(repo, &["remote"])?;
    Ok(stdout_string(&out)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Working-tree change counts.
pub fn working_tree(repo: &Path) -> Result<WorkingTree> {
    let out = git(repo, &["status", "--porcelain=v2", "-z"])?;
    Ok(parse_porcelain_v2_z(stdout_string(&out).as_str()))
}

/// Ahead/behind of `branch` vs `remote/branch`, using the *local* remote-tracking
/// ref (no network). `None` when that remote branch doesn't exist locally.
pub fn ahead_behind(repo: &Path, branch: &str, remote: &str) -> Result<Option<AheadBehind>> {
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let verify = git(repo, &["rev-parse", "--verify", "--quiet", &tracking])?;
    if !verify.status.success() {
        return Ok(None);
    }
    let spec = format!("{branch}...{remote}/{branch}");
    let out = git(repo, &["rev-list", "--left-right", "--count", &spec])?;
    if !out.status.success() {
        return Ok(None);
    }
    let text = stdout_string(&out);
    let mut nums = text.split_whitespace();
    let ahead = nums.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    let behind = nums.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    Ok(Some(AheadBehind { ahead, behind }))
}

/// Trial-merge `branch` with `remote/branch` via `git merge-tree --write-tree`
/// (git ≥ 2.38) without touching the working tree. `true` = conflicts.
/// Only meaningful for diverged branches.
pub fn would_conflict(repo: &Path, branch: &str, remote: &str) -> Result<bool> {
    let upstream = format!("{remote}/{branch}");
    let out = git(repo, &["merge-tree", "--write-tree", branch, &upstream])?;
    // exit 0 = clean merge, exit 1 = conflicts.
    Ok(!out.status.success())
}

/// Full sync classification of `branch` vs `remote/branch`: ahead/behind from
/// local tracking refs, plus a conflict probe only when diverged.
pub fn branch_sync(repo: &Path, branch: &str, remote: &str) -> Result<BranchSync> {
    let ab = ahead_behind(repo, branch, remote)?;
    let diverged = matches!(ab, Some(v) if v.ahead > 0 && v.behind > 0);
    let conflict = if diverged {
        Some(would_conflict(repo, branch, remote)?)
    } else {
        None
    };
    Ok(BranchSync::classify(ab, conflict))
}

/// Result of one `git push` attempt.
pub struct PushOutcome {
    pub success: bool,
    pub stderr: String,
}

/// Push `branch` to `remote`. Never forces (plain push — git rejects non-fast-
/// forwards on its own, which is our backstop). `dry_run` maps to `--dry-run`;
/// `tags` adds `--follow-tags` (annotated tags reachable from pushed commits).
pub fn push(
    repo: &Path,
    remote: &str,
    branch: &str,
    dry_run: bool,
    tags: bool,
) -> Result<PushOutcome> {
    let mut args = vec!["push"];
    if dry_run {
        args.push("--dry-run");
    }
    if tags {
        args.push("--follow-tags");
    }
    args.push(remote);
    args.push(branch);
    let out = git(repo, &args)?;
    Ok(PushOutcome {
        success: out.status.success(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}
