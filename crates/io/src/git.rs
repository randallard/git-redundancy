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

/// Configured URL for `remote` (its fetch URL), or `None` when the remote is
/// absent. Used to recover a repo's home name (ADR-0012).
pub fn remote_url(repo: &Path, remote: &str) -> Result<Option<String>> {
    let out = git(repo, &["remote", "get-url", remote])?;
    if out.status.success() {
        let url = stdout_string(&out).trim().to_string();
        Ok((!url.is_empty()).then_some(url))
    } else {
        Ok(None)
    }
}

/// Committer date of `HEAD` as `YYYY-MM-DD`, or `None` when the repo has no
/// commits yet. Used for the `gr onboard` per-repo context line (ADR-0017).
pub fn last_commit_date(repo: &Path) -> Result<Option<String>> {
    let out = git(repo, &["log", "-1", "--format=%cs"])?;
    if out.status.success() {
        let d = stdout_string(&out).trim().to_string();
        Ok((!d.is_empty()).then_some(d))
    } else {
        Ok(None) // no commits on HEAD yet
    }
}

/// Does the repo have at least one commit reachable from `HEAD`? A commitless
/// repo can't be onboarded as-is (ADR-0017 pre-flight).
pub fn has_commits(repo: &Path) -> Result<bool> {
    Ok(git(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?
        .status
        .success())
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

// --- mutating local ops for the lifecycle commands (ADR-0013) ----------------

/// Outcome of a mutating git command: success + captured stderr (first line is
/// what the CLI surfaces on failure).
pub struct CmdOutcome {
    pub success: bool,
    pub stderr: String,
}

fn outcome(out: Output) -> CmdOutcome {
    CmdOutcome {
        success: out.status.success(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    }
}

/// `git clone <url> <dir>`. Run without `-C` (the target doesn't exist yet).
pub fn clone(url: &str, dir: &Path) -> Result<CmdOutcome> {
    let out = Command::new("git")
        .arg("clone")
        .arg(url)
        .arg(dir)
        .output()
        .with_context(|| format!("cloning {url} into {}", dir.display()))?;
    Ok(outcome(out))
}

/// Add remote `name` → `url`.
pub fn add_remote(repo: &Path, name: &str, url: &str) -> Result<CmdOutcome> {
    Ok(outcome(git(repo, &["remote", "add", name, url])?))
}

/// Repoint an existing remote `name` at `url`.
pub fn set_remote_url(repo: &Path, name: &str, url: &str) -> Result<CmdOutcome> {
    Ok(outcome(git(repo, &["remote", "set-url", name, url])?))
}

/// Remove remote `name`. A missing remote is not an error (idempotent cleanup,
/// e.g. dropping the clone-minted `origin`).
pub fn remove_remote(repo: &Path, name: &str) -> Result<()> {
    let _ = git(repo, &["remote", "remove", name])?;
    Ok(())
}

/// `git fetch <remote>` — refresh remote-tracking refs before classifying.
pub fn fetch(repo: &Path, remote: &str) -> Result<CmdOutcome> {
    Ok(outcome(git(repo, &["fetch", remote])?))
}

/// Branch names on a remote via `git ls-remote --heads <url>` (no clone needed),
/// for listing a home's branches in the detail view (ADR-0014). `Err` if the
/// remote is unreachable.
pub fn ls_remote_heads(url: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .args(["ls-remote", "--heads", url])
        .output()
        .with_context(|| format!("git ls-remote {url}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "ls-remote {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split('\t').nth(1)) // "<sha>\trefs/heads/<branch>"
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .map(str::to_string)
        .collect())
}

/// SHA of `refs/heads/<branch>` on a remote via `git ls-remote`, or `None` when
/// the remote has no such branch. Used to confirm the backup fast-forwards from
/// the new primary during `repoint` (ADR-0018).
pub fn ls_remote_sha(url: &str, branch: &str) -> Result<Option<String>> {
    let refname = format!("refs/heads/{branch}");
    let out = Command::new("git")
        .args(["ls-remote", url, &refname])
        .output()
        .with_context(|| format!("git ls-remote {url} {refname}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "ls-remote {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|l| l.split('\t').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

/// Is `ancestor` an ancestor of (or equal to) `descendant`? `git merge-base
/// --is-ancestor` over local objects (no network). Used to verify the backup is
/// fast-forward-consistent with the new primary before rewiring (ADR-0018).
pub fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    Ok(
        git(repo, &["merge-base", "--is-ancestor", ancestor, descendant])?
            .status
            .success(),
    )
}

/// Fast-forward the **current** branch to `<remote>/<branch>` via
/// `git merge --ff-only` — never a merge commit, never a non-ff. Caller must
/// ensure `branch` is checked out and the tree is clean.
pub fn ff_merge_current(repo: &Path, remote: &str, branch: &str) -> Result<CmdOutcome> {
    let upstream = format!("{remote}/{branch}");
    Ok(outcome(git(repo, &["merge", "--ff-only", &upstream])?))
}

/// Fast-forward a **non-current** local branch ref to its tracking ref without a
/// checkout, via `git fetch . <remote>/<branch>:<branch>`. Branch-ref updates are
/// fast-forward-only (git rejects non-ff without `--force`), so this is safe and
/// never touches the working tree.
pub fn ff_update_branch(repo: &Path, remote: &str, branch: &str) -> Result<CmdOutcome> {
    let refspec = format!("refs/remotes/{remote}/{branch}:refs/heads/{branch}");
    Ok(outcome(git(repo, &["fetch", ".", &refspec])?))
}
