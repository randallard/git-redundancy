//! `gr push` — push easy (fast-forward / new-branch), **committed** work home.
//!
//! Honors ADR-0006: never auto-commits, never force-pushes, never touches a
//! diverged or behind branch (those are skipped and reported). Dirty working
//! trees are surfaced loudly but don't block pushing already-committed commits.
//! Honors ADR-0009: with `[transport].auto`, the `order` remotes are treated as
//! interchangeable paths to the same server — push tries them in order until one
//! succeeds (LAN first, Tailscale fallback).

use crate::PushArgs;
use anyhow::Result;
use git_redundancy_core::BranchSync;
use git_redundancy_io::{config::Config, discovery::discover, git, Audit};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Default)]
struct Tally {
    pushed: u32,
    uptodate: u32,
    skipped: u32,
    failed: u32,
    dirty: u32,
}

pub fn run_push(args: &PushArgs) -> Result<()> {
    let cfg = Config::load()?;
    if cfg.is_empty() {
        println!(
            "No repos configured. Add roots/repos to {}.",
            Config::config_path().display()
        );
        return Ok(());
    }

    let repos: Vec<_> = discover(&cfg)
        .into_iter()
        .filter(|r| repo_selected(r, &args.only))
        .collect();
    if repos.is_empty() {
        println!("No repos match.");
        return Ok(());
    }

    let (order, failover) = remote_plan(args, &cfg);
    let audit = Audit::from_config(&cfg);
    if args.dry_run {
        println!("[dry-run] no remote will be updated (not audited)\n");
    }

    let mut tally = Tally::default();
    for repo in &repos {
        let name = repo_name(repo);
        let current = git::current_branch(repo)?;
        let wt = git::working_tree(repo)?;
        let repo_remotes: BTreeSet<String> = git::remotes(repo)?.into_iter().collect();

        // Candidate remotes for this repo, in plan order (or all of the repo's own
        // remotes if nothing was configured).
        let candidates: Vec<String> = if order.is_empty() {
            repo_remotes.iter().cloned().collect()
        } else {
            order
                .iter()
                .filter(|r| repo_remotes.contains(*r))
                .cloned()
                .collect()
        };
        if candidates.is_empty() {
            line(&name, "—", "—", "no configured home remote on this repo");
            tally.skipped += 1;
            continue;
        }

        let branches: Vec<String> = if args.all_branches {
            git::local_branches(repo)?
        } else {
            current.clone().into_iter().collect()
        };
        if branches.is_empty() {
            line(
                &name,
                "(detached)",
                "—",
                "no branch to push (detached HEAD)",
            );
            tally.skipped += 1;
            continue;
        }

        for branch in &branches {
            if failover {
                decide_and_push(
                    repo,
                    &name,
                    branch,
                    &candidates[0],
                    &candidates,
                    args,
                    &audit,
                    &mut tally,
                )?;
            } else {
                for r in &candidates {
                    decide_and_push(
                        repo,
                        &name,
                        branch,
                        r,
                        std::slice::from_ref(r),
                        args,
                        &audit,
                        &mut tally,
                    )?;
                }
            }
        }

        // Surface dirty state once per repo — committed work was still pushed.
        if wt.has_uncommitted_changes() || wt.untracked > 0 {
            let mut parts = Vec::new();
            if wt.staged > 0 {
                parts.push(format!("{} staged", wt.staged));
            }
            if wt.unstaged > 0 {
                parts.push(format!("{} unstaged", wt.unstaged));
            }
            if wt.conflicts > 0 {
                parts.push(format!("{} conflicts", wt.conflicts));
            }
            if wt.untracked > 0 {
                parts.push(format!("{} untracked", wt.untracked));
            }
            println!(
                "  ⚠ {name}: {} — NOT backed up (commit to include)",
                parts.join(", ")
            );
            tally.dirty += 1;
        }
    }

    let dirty = if tally.dirty > 0 {
        format!(" · {} dirty", tally.dirty)
    } else {
        String::new()
    };
    println!(
        "\n{} pushed · {} up-to-date · {} skipped · {} failed{}",
        tally.pushed, tally.uptodate, tally.skipped, tally.failed, dirty
    );
    if !args.dry_run {
        if let Some(p) = audit.path() {
            println!("audit log: {}", p.display());
        }
    }
    if tally.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Classify `branch` vs `refr`, then push only if easy — trying `attempt` remotes
/// in order until one succeeds (failover).
#[allow(clippy::too_many_arguments)]
fn decide_and_push(
    repo: &Path,
    name: &str,
    branch: &str,
    refr: &str,
    attempt: &[String],
    args: &PushArgs,
    audit: &Audit,
    tally: &mut Tally,
) -> Result<()> {
    let sync = git::branch_sync(repo, branch, refr)?;
    match sync {
        BranchSync::UpToDate => {
            line(name, branch, refr, "up-to-date");
            audit_log(audit, args, name, branch, refr, "up-to-date", "");
            tally.uptodate += 1;
        }
        BranchSync::Behind(n) => {
            let detail = format!("behind {n}");
            line(
                name,
                branch,
                refr,
                &format!("SKIPPED: {detail} (pull first)"),
            );
            audit_log(audit, args, name, branch, refr, "skipped", &detail);
            tally.skipped += 1;
        }
        BranchSync::Diverged {
            ahead,
            behind,
            conflict,
        } => {
            let why = if conflict {
                "diverged + CONFLICT"
            } else {
                "diverged"
            };
            line(
                name,
                branch,
                refr,
                &format!("SKIPPED: {why} (↑{ahead} ↓{behind}; never forced)"),
            );
            audit_log(
                audit,
                args,
                name,
                branch,
                refr,
                "skipped",
                &format!("{why} ↑{ahead} ↓{behind}"),
            );
            tally.skipped += 1;
        }
        easy @ (BranchSync::NoRemoteBranch | BranchSync::Ahead(_)) => {
            let what = match easy {
                BranchSync::Ahead(n) => format!("↑{n}"),
                _ => "new".to_string(),
            };
            let mut done = false;
            let mut last_err = String::new();
            for r in attempt {
                let outcome = git::push(repo, r, branch, args.dry_run, args.tags)?;
                if outcome.success {
                    let verb = if args.dry_run { "would push" } else { "pushed" };
                    line(name, branch, r, &format!("{verb} ({what})"));
                    audit_log(audit, args, name, branch, r, "pushed", &what);
                    tally.pushed += 1;
                    done = true;
                    break;
                }
                last_err = first_line(&outcome.stderr);
            }
            if !done {
                line(name, branch, refr, &format!("FAILED ({what}): {last_err}"));
                audit_log(
                    audit,
                    args,
                    name,
                    branch,
                    refr,
                    "failed",
                    &format!("{what}: {last_err}"),
                );
                tally.failed += 1;
            }
        }
    }
    Ok(())
}

/// Append an audit record for a real action. Dry-run performs no action, so it is
/// not audited. A failure to write the security log is surfaced loudly but does
/// not abort work already done.
#[allow(clippy::too_many_arguments)]
fn audit_log(
    audit: &Audit,
    args: &PushArgs,
    repo: &str,
    branch: &str,
    remote: &str,
    result: &str,
    detail: &str,
) {
    if args.dry_run {
        return;
    }
    if let Err(e) = audit.record(repo, branch, remote, result, detail) {
        eprintln!("  ⚠ audit log write failed: {e}");
    }
}

/// `--remote` wins (single, no failover). Else an explicit `[transport].order` is
/// the interchangeable failover group (honoring `auto`). Else `default_remotes`,
/// pushed independently. Else (empty) fall back to each repo's own remotes.
fn remote_plan(args: &PushArgs, cfg: &Config) -> (Vec<String>, bool) {
    if let Some(r) = &args.remote {
        return (vec![r.clone()], false);
    }
    if !cfg.transport.order.is_empty() {
        return (cfg.transport.order.clone(), cfg.transport.auto);
    }
    if !cfg.default_remotes.is_empty() {
        return (cfg.default_remotes.clone(), false);
    }
    (Vec::new(), false)
}

fn repo_selected(repo: &Path, only: &[String]) -> bool {
    only.is_empty()
        || only
            .iter()
            .any(|o| repo_name(repo) == *o || repo.to_string_lossy() == o.as_str())
}

fn repo_name(repo: &Path) -> String {
    repo.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.display().to_string())
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn line(name: &str, branch: &str, remote: &str, status: &str) {
    println!("  {name:<18} {branch:<22} {remote:<9} {status}");
}
