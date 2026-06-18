//! `gr create` / `gr clone` / `gr sync` — the ADR-0013 lifecycle commands that
//! close the local↔home gap. All three require the server reachable and fail
//! loudly without half-acting; all audit their actions (ADR-0004 AU); none ever
//! force-push, auto-commit, or auto-merge.

use crate::{CloneArgs, CreateArgs, SyncArgs};
use anyhow::{Context, Result};
use git_redundancy_core::{BranchSync, SyncAction};
use git_redundancy_io::{config::Config, discovery::discover, git, server, Audit};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

fn repo_name(repo: &Path) -> String {
    repo.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.display().to_string())
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

/// Shared "[server] not configured" guidance.
fn require_server(cfg: &Config) -> Result<()> {
    if cfg.server_enabled() {
        return Ok(());
    }
    anyhow::bail!(
        "no [server] configured in {} — add `[server]\\nroot = \"/data/git\"` to enable lifecycle commands",
        Config::config_path().display()
    );
}

// ============================ gr create =====================================

/// `gr create [name]` — make a bare home for the current working copy, wire the
/// remotes, and push. Refuses if a home of that name already exists.
pub fn run_create(args: &CreateArgs) -> Result<()> {
    let cfg = Config::load()?;
    require_server(&cfg)?;

    let cwd = std::env::current_dir().context("getting current directory")?;
    git::current_branch(&cwd)?
        .as_deref()
        .filter(|b| !b.is_empty())
        .context("not on a branch (detached HEAD) — checkout a branch before `create`")?;
    let branch = git::current_branch(&cwd)?.unwrap();
    let name = args.name.clone().unwrap_or_else(|| repo_name(&cwd));

    let repos = discover(&cfg);
    let root = cfg.server.root.clone();
    let alias = server::pick_alias(&cfg, &repos)?;

    if server::home_exists(&alias, &root, &name)? {
        anyhow::bail!(
            "a home named `{name}` already exists on the server — use `gr sync` to back it up"
        );
    }

    println!(
        "creating bare home {}/{name}.git via {alias} …",
        root.display()
    );
    let init = server::init_bare(&alias, &root, &name)?;
    if !init.success {
        anyhow::bail!("could not create bare repo: {}", first_line(&init.stderr));
    }
    // Match the home's default branch to what we push (the empty-looking-bare gotcha).
    let _ = server::set_head(&alias, &root, &name, &branch)?;

    // Wire data / data-lan per ADR-0009, replacing any stale URL.
    for (remote, url) in server::remote_wiring(&cfg, &repos, &root, &name, &alias) {
        if git::remote_url(&cwd, &remote)?.is_some() {
            git::set_remote_url(&cwd, &remote, &url)?;
        } else {
            git::add_remote(&cwd, &remote, &url)?;
        }
        println!("  remote {remote} → {url}");
    }

    // Push the branch (or all with -a) over the live alias's remote.
    let audit = Audit::from_config(&cfg);
    let push_remote = primary_remote(&cfg, &cwd)?;
    let branches = if args.all_branches {
        git::local_branches(&cwd)?
    } else {
        vec![branch.clone()]
    };
    let mut failed = false;
    for b in &branches {
        let out = git::push(&cwd, &push_remote, b, false, false)?;
        if out.success {
            println!("  pushed {b} → {push_remote}");
            let _ = audit.record(&name, b, &push_remote, "created", "");
        } else {
            eprintln!("  push {b} failed: {}", first_line(&out.stderr));
            failed = true;
        }
    }
    if failed {
        std::process::exit(1);
    }
    println!("created `{name}` ({} branch(es) pushed)", branches.len());
    Ok(())
}

/// The first transport remote actually present on the repo (the one to push to).
fn primary_remote(cfg: &Config, repo: &Path) -> Result<String> {
    let order = if cfg.transport.order.is_empty() {
        cfg.default_remotes.clone()
    } else {
        cfg.transport.order.clone()
    };
    let have: BTreeSet<String> = git::remotes(repo)?.into_iter().collect();
    order
        .into_iter()
        .find(|r| have.contains(r))
        .context("no transport remote present after wiring")
}

// ============================ gr clone ======================================

/// `gr clone <name> [dir]` — clone a home-only repo into a configured root, wire
/// the remotes, and drop the clone-minted `origin` (kept cloud-only).
pub fn run_clone(args: &CloneArgs) -> Result<()> {
    let cfg = Config::load()?;
    require_server(&cfg)?;
    let name = &args.name;
    let root = cfg.server.root.clone();

    let dir = match clone_target(&cfg, args)? {
        Some(d) => d,
        None => return Ok(()), // guidance already printed; user's move.
    };

    let repos = discover(&cfg);
    let alias = server::pick_alias(&cfg, &repos)?;
    if !server::home_exists(&alias, &root, name)? {
        anyhow::bail!("no home named `{name}` on the server — `gr create` makes one");
    }

    let url = server::home_url(&alias, &root, name);
    println!("cloning {url} → {} …", dir.display());
    let out = git::clone(&url, &dir)?;
    if !out.success {
        anyhow::bail!("clone failed: {}", first_line(&out.stderr));
    }

    // Drop the clone's `origin` (reserved for the DCN cloud), then wire data/data-lan.
    git::remove_remote(&dir, "origin")?;
    for (remote, rurl) in server::remote_wiring(&cfg, &repos, &root, name, &alias) {
        if git::remote_url(&dir, &remote)?.is_some() {
            git::set_remote_url(&dir, &remote, &rurl)?;
        } else {
            git::add_remote(&dir, &remote, &rurl)?;
        }
    }
    let audit = Audit::from_config(&cfg);
    let _ = audit.record(name, "-", &alias, "cloned", &dir.display().to_string());
    println!(
        "cloned `{name}` into {} (remotes wired, origin dropped)",
        dir.display()
    );
    Ok(())
}

/// Resolve the clone target dir, enforcing that it lands inside a configured
/// root (ADR-0013). Returns `None` (after printing guidance) when it doesn't.
fn clone_target(cfg: &Config, args: &CloneArgs) -> Result<Option<PathBuf>> {
    let under_a_root = |d: &Path| cfg.roots.iter().any(|r| d.starts_with(r) && d != r);

    let dir = match &args.dir {
        Some(d) => d.clone(),
        None => match cfg.roots.first() {
            Some(r) => r.join(&args.name),
            None => {
                println!("No roots configured to clone into. Add one to {} and retry:\n\n  roots = [\"/data/Development\"]",
                    Config::config_path().display());
                return Ok(None);
            }
        },
    };

    if !under_a_root(&dir) {
        println!(
            "Target {} is not inside a configured root, so `gr` wouldn't discover it.\nConfigured roots:",
            dir.display()
        );
        for r in &cfg.roots {
            println!("  {}", r.display());
        }
        println!(
            "\nAdd a root to {} (or pass a dir under one) and retry — your move.",
            Config::config_path().display()
        );
        return Ok(None);
    }
    Ok(Some(dir))
}

// ============================ gr sync =======================================

#[derive(Default)]
struct Tally {
    pushed: u32,
    pulled: u32,
    uptodate: u32,
    skipped: u32,
    failed: u32,
}

/// `gr sync [repos…]` — reconcile easy work both ways: easy-push ahead/new,
/// fast-forward-pull behind (clean tree only), report diverged/conflict.
pub fn run_sync(args: &SyncArgs) -> Result<()> {
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
        .filter(|r| args.repos.is_empty() || args.repos.contains(&repo_name(r)))
        .collect();
    if repos.is_empty() {
        println!("No repos match.");
        return Ok(());
    }

    let order = if cfg.transport.order.is_empty() {
        cfg.default_remotes.clone()
    } else {
        cfg.transport.order.clone()
    };
    let audit = Audit::from_config(&cfg);
    if args.dry_run {
        println!("[dry-run] nothing will be changed (not audited)\n");
    }

    let mut tally = Tally::default();
    for repo in &repos {
        sync_repo(repo, &order, args, &audit, &mut tally)?;
    }

    println!(
        "\n{} pushed · {} pulled · {} up-to-date · {} skipped · {} failed",
        tally.pushed, tally.pulled, tally.uptodate, tally.skipped, tally.failed
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

fn sync_repo(
    repo: &Path,
    order: &[String],
    args: &SyncArgs,
    audit: &Audit,
    tally: &mut Tally,
) -> Result<()> {
    let name = repo_name(repo);
    let have: BTreeSet<String> = git::remotes(repo)?.into_iter().collect();
    let candidates: Vec<String> = order
        .iter()
        .filter(|r| have.contains(*r))
        .cloned()
        .collect();
    if candidates.is_empty() {
        line(&name, "—", "—", "no configured home remote");
        tally.skipped += 1;
        return Ok(());
    }

    // Refresh tracking refs over the first reachable candidate — that's our live
    // transport for this repo (LAN→Tailscale failover, ADR-0009).
    let live = match first_reachable(repo, &candidates)? {
        Some(r) => r,
        None => {
            line(&name, "—", "—", "unreachable (no transport fetched)");
            tally.skipped += 1;
            return Ok(());
        }
    };

    let current = git::current_branch(repo)?;
    let wt = git::working_tree(repo)?;
    let branches: Vec<String> = if args.all_branches {
        git::local_branches(repo)?
    } else {
        current.clone().into_iter().collect()
    };
    if branches.is_empty() {
        line(&name, "(detached)", "—", "no branch to sync");
        tally.skipped += 1;
        return Ok(());
    }

    for branch in &branches {
        let is_current = current.as_deref() == Some(branch.as_str());
        // Only the current branch's fast-forward touches the working tree.
        let tree_clean = if is_current { wt.is_clean() } else { true };
        let sync = git::branch_sync(repo, branch, &live)?;
        let action = SyncAction::plan(sync, tree_clean);
        act(
            repo,
            &name,
            branch,
            &live,
            is_current,
            action,
            &candidates,
            args,
            audit,
            tally,
        )?;
    }
    Ok(())
}

/// Fetch each candidate in order; the first that succeeds is the live remote.
fn first_reachable(repo: &Path, candidates: &[String]) -> Result<Option<String>> {
    for r in candidates {
        if git::fetch(repo, r)?.success {
            return Ok(Some(r.clone()));
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn act(
    repo: &Path,
    name: &str,
    branch: &str,
    live: &str,
    is_current: bool,
    action: SyncAction,
    candidates: &[String],
    args: &SyncArgs,
    audit: &Audit,
    tally: &mut Tally,
) -> Result<()> {
    match action {
        SyncAction::UpToDate => {
            line(name, branch, live, "up-to-date");
            tally.uptodate += 1;
        }
        SyncAction::Report => {
            let detail = match git::branch_sync(repo, branch, live)? {
                BranchSync::Diverged { conflict: true, .. } => "diverged + CONFLICT (manual)",
                _ => "diverged (manual)",
            };
            line(name, branch, live, &format!("SKIPPED: {detail}"));
            audit_action(audit, args, name, branch, live, "skipped", detail);
            tally.skipped += 1;
        }
        SyncAction::BlockedDirty(n) => {
            let detail = format!("behind {n} but tree dirty — commit/stash to fast-forward");
            line(name, branch, live, &format!("SKIPPED: {detail}"));
            audit_action(audit, args, name, branch, live, "skipped", &detail);
            tally.skipped += 1;
        }
        SyncAction::Push => {
            if !confirm(args, name, branch, "push")? {
                line(name, branch, live, "skipped (cancelled)");
                tally.skipped += 1;
                return Ok(());
            }
            push_failover(repo, name, branch, candidates, args, audit, tally)?;
        }
        SyncAction::FastForward(n) => {
            if !confirm(args, name, branch, &format!("fast-forward ↓{n}"))? {
                line(name, branch, live, "skipped (cancelled)");
                tally.skipped += 1;
                return Ok(());
            }
            if args.dry_run {
                line(name, branch, live, &format!("would fast-forward (↓{n})"));
                tally.pulled += 1;
                return Ok(());
            }
            let out = if is_current {
                git::ff_merge_current(repo, live, branch)?
            } else {
                git::ff_update_branch(repo, live, branch)?
            };
            if out.success {
                line(name, branch, live, &format!("fast-forwarded (↓{n})"));
                audit_action(audit, args, name, branch, live, "ff-pull", &format!("↓{n}"));
                tally.pulled += 1;
            } else {
                line(
                    name,
                    branch,
                    live,
                    &format!("FAILED ff: {}", first_line(&out.stderr)),
                );
                audit_action(
                    audit,
                    args,
                    name,
                    branch,
                    live,
                    "failed",
                    &first_line(&out.stderr),
                );
                tally.failed += 1;
            }
        }
    }
    Ok(())
}

/// Easy-push trying candidates in order until one succeeds (failover).
#[allow(clippy::too_many_arguments)]
fn push_failover(
    repo: &Path,
    name: &str,
    branch: &str,
    candidates: &[String],
    args: &SyncArgs,
    audit: &Audit,
    tally: &mut Tally,
) -> Result<()> {
    let mut last_err = String::new();
    for r in candidates {
        let out = git::push(repo, r, branch, args.dry_run, false)?;
        if out.success {
            let verb = if args.dry_run { "would push" } else { "pushed" };
            line(name, branch, r, verb);
            audit_action(audit, args, name, branch, r, "pushed", "");
            tally.pushed += 1;
            return Ok(());
        }
        last_err = first_line(&out.stderr);
    }
    line(
        name,
        branch,
        &candidates[0],
        &format!("FAILED push: {last_err}"),
    );
    audit_action(
        audit,
        args,
        name,
        branch,
        &candidates[0],
        "failed",
        &last_err,
    );
    tally.failed += 1;
    Ok(())
}

/// Under `-i`, prompt to confirm an effecting action; otherwise always yes.
fn confirm(args: &SyncArgs, name: &str, branch: &str, what: &str) -> Result<bool> {
    if !args.interactive || args.dry_run {
        return Ok(true);
    }
    print!("  {name} {branch}: {what}? [y/N] ");
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes"))
}

fn audit_action(
    audit: &Audit,
    args: &SyncArgs,
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

fn line(name: &str, branch: &str, remote: &str, status: &str) {
    println!("  {name:<18} {branch:<22} {remote:<9} {status}");
}
