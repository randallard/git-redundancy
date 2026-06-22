//! `gr create` / `gr clone` / `gr sync` — the ADR-0013 lifecycle commands that
//! close the local↔home gap. All three require the server reachable and fail
//! loudly without half-acting; all audit their actions (ADR-0004 AU); none ever
//! force-push, auto-commit, or auto-merge.

use crate::{CloneArgs, CreateArgs, OnboardArgs, SyncArgs};
use anyhow::{Context, Result};
use git_redundancy_core::presence::Lifecycle;
use git_redundancy_core::{BranchSync, SyncAction};
use git_redundancy_io::{config::Config, discovery::discover, git, server, Audit};
use std::collections::{BTreeMap, BTreeSet};
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
    let name = args.name.clone().unwrap_or_else(|| repo_name(&cwd));

    let repos = discover(&cfg);
    let alias = server::pick_alias(&cfg, &repos)?;
    let audit = Audit::from_config(&cfg);

    let outcome = create_home(&cfg, &cwd, &name, args.all_branches, &repos, &alias, &audit)?;
    if outcome.failed {
        std::process::exit(1);
    }
    Ok(())
}

/// Result of provisioning a home: whether any branch push failed. The caller
/// decides whether that's fatal — `create` exits, `onboard` reports and walks on.
struct CreateOutcome {
    failed: bool,
}

/// The ADR-0013 + ADR-0016 core of `create`: provision a brand-new home for
/// `repo` (named `name`), install the replication topology, wire the remotes,
/// and push. Shared by `gr create` (the cwd) and `gr onboard` (any repo path).
/// Refuses if a primary home of that name already exists.
fn create_home(
    cfg: &Config,
    repo: &Path,
    name: &str,
    all_branches: bool,
    repos: &[PathBuf],
    alias: &str,
    audit: &Audit,
) -> Result<CreateOutcome> {
    let branch = git::current_branch(repo)?
        .filter(|b| !b.is_empty())
        .context("not on a branch (detached HEAD) — checkout a branch before onboarding")?;
    let root = cfg.server.root.clone();

    if server::home_exists(alias, &root, name)? {
        anyhow::bail!(
            "a home named `{name}` already exists on the server — use `gr sync` to back it up"
        );
    }

    println!(
        "creating bare home {}/{name}.git via {alias} …",
        root.display()
    );
    let init = server::init_bare(alias, &root, name)?;
    if !init.success {
        anyhow::bail!("could not create bare repo: {}", first_line(&init.stderr));
    }
    // Match the home's default branch to what we push (the empty-looking-bare gotcha).
    let _ = server::set_head(alias, &root, name, &branch)?;

    // ADR-0016: provision the full fleet topology so onboarding yields *redundancy*,
    // not just a primary home. Install the primary's replication hook and create +
    // harden the backup home — both before the push, so the push mirrors immediately.
    // gr provisions; the primary→backup mirror itself stays the controller's job.
    if cfg.backup_enabled() {
        let hook = server::install_hook(
            alias,
            &root,
            name,
            "post-receive",
            server::POST_RECEIVE_HOOK,
        )?;
        if !hook.success {
            anyhow::bail!(
                "created the primary home but failed to install its post-receive hook: {}",
                first_line(&hook.stderr)
            );
        }
        println!("  installed post-receive hook on {alias}");

        let bk_alias = server::pick_backup_alias(cfg)?;
        let bk_root = cfg.backup.root.clone();
        if !server::home_exists(&bk_alias, &bk_root, name)? {
            let bk = server::init_bare(&bk_alias, &bk_root, name)?;
            if !bk.success {
                anyhow::bail!(
                    "created the primary home but failed to create the backup home on {bk_alias}: {}",
                    first_line(&bk.stderr)
                );
            }
            let _ = server::set_head(&bk_alias, &bk_root, name, &branch)?;
        }
        let hard = server::harden_home(&bk_alias, &bk_root, name)?;
        if !hard.success {
            anyhow::bail!(
                "backup home exists but could not be hardened on {bk_alias}: {}",
                first_line(&hard.stderr)
            );
        }
        let pre = server::install_hook(
            &bk_alias,
            &bk_root,
            name,
            "pre-receive",
            server::PRE_RECEIVE_HOOK,
        )?;
        if !pre.success {
            anyhow::bail!(
                "backup home exists but its pre-receive guard failed to install on {bk_alias}: {}",
                first_line(&pre.stderr)
            );
        }
        println!("  backup home ready + hardened on {bk_alias}");
    } else {
        println!(
            "  ⚠ no [backup] configured — this repo will live on the primary only, NOT redundant"
        );
    }

    // Wire data / data-lan per ADR-0009, replacing any stale URL.
    for (remote, url) in server::remote_wiring(cfg, repos, &root, name, alias) {
        if git::remote_url(repo, &remote)?.is_some() {
            git::set_remote_url(repo, &remote, &url)?;
        } else {
            git::add_remote(repo, &remote, &url)?;
        }
        println!("  remote {remote} → {url}");
    }

    // Push the branch (or all with -a) over the live alias's remote.
    let push_remote = primary_remote(cfg, repo)?;
    let branches = if all_branches {
        git::local_branches(repo)?
    } else {
        vec![branch.clone()]
    };
    let mut failed = false;
    for b in &branches {
        let out = git::push(repo, &push_remote, b, false, false)?;
        if out.success {
            println!("  pushed {b} → {push_remote}");
            let _ = audit.record(name, b, &push_remote, "created", "");
        } else {
            eprintln!("  push {b} failed: {}", first_line(&out.stderr));
            failed = true;
        }
    }
    let topo = if cfg.backup_enabled() {
        " — redundant (primary + backup)"
    } else {
        " — primary only (no [backup])"
    };
    println!(
        "created `{name}` ({} branch(es) pushed){topo}",
        branches.len()
    );
    Ok(CreateOutcome { failed })
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

// ============================ gr onboard ====================================

/// What a candidate needs to become redundant.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Onboard {
    /// Local-only, no home anywhere → `create` provisions the full topology.
    Create,
    /// Local-only on the primary but a home already exists on the *backup* (the
    /// original-7 sub-state) → needs `repoint` (ADR-0018), not a fresh create.
    Repoint,
}

#[derive(Default)]
struct OnboardTally {
    onboarded: u32,
    ignored: u32,
    skipped: u32,
    failed: u32,
}

/// `gr onboard` — walk the un-redundant, non-ignored repos one at a time and let
/// the operator decide each: onboard (y) / ignore (n) / skip (s) / quit (q), plus
/// repoint (r) for backup-only homes (ADR-0017). Every y/n is committed as it
/// happens, so quitting keeps all decisions and the walk is resumable.
pub fn run_onboard(args: &OnboardArgs) -> Result<()> {
    let cfg = Config::load()?;
    require_server(&cfg)?;
    if cfg.is_empty() {
        println!(
            "No repos configured. Add roots/repos to {}.",
            Config::config_path().display()
        );
        return Ok(());
    }

    let repos = discover(&cfg);
    let survey = git_redundancy_io::survey(&cfg);
    if !survey.reachable {
        anyhow::bail!(
            "home server unreachable — onboarding provisions on it, so it must be up. \
             Use `gr status --offline` to inspect locally, then retry."
        );
    }
    let path_by_dir: BTreeMap<String, PathBuf> =
        repos.iter().map(|p| (repo_name(p), p.clone())).collect();

    // Homes that exist on the backup — used to spot the repoint sub-state.
    let backup_homes: BTreeSet<&str> = survey
        .backup
        .as_ref()
        .filter(|b| b.reachable)
        .map(|b| b.homes.iter().map(String::as_str).collect())
        .unwrap_or_default();

    // Candidates: local working copy present, no primary home yet, not ignored.
    // (linked = done; home-only = `clone`'s job; ignored = deliberately skipped.)
    let mut candidates: Vec<(String, PathBuf, Onboard)> = Vec::new();
    for p in &survey.presences {
        if p.lifecycle != Lifecycle::LocalOnly {
            continue;
        }
        let Some(dir) = &p.local_dir else { continue };
        if cfg.is_ignored(&p.home_name) || cfg.is_ignored(dir) {
            continue;
        }
        let Some(repo) = path_by_dir.get(dir) else {
            continue;
        };
        let kind = if backup_homes.contains(p.home_name.as_str()) {
            Onboard::Repoint
        } else {
            Onboard::Create
        };
        candidates.push((p.home_name.clone(), repo.clone(), kind));
    }

    if candidates.is_empty() {
        println!("Nothing to onboard — every repo is redundant or ignored. ✓");
        return Ok(());
    }

    let alias = server::pick_alias(&cfg, &repos)?;
    let audit = Audit::from_config(&cfg);
    let total = candidates.len();
    if args.dry_run {
        println!("[dry-run] {total} candidate(s); showing the plan, changing nothing.\n");
    }

    let mut tally = OnboardTally::default();
    for (i, (name, repo, kind)) in candidates.iter().enumerate() {
        print_candidate(i + 1, total, name, repo)?;

        // Pre-flight: a detached HEAD or a commitless repo can't be onboarded
        // as-is — flag it instead of erroring partway through `create`.
        let on_branch = git::current_branch(repo)?.is_some_and(|b| !b.is_empty());
        let has_commits = git::has_commits(repo)?;
        let blocked = !on_branch || !has_commits;
        let blocked_why = if !has_commits {
            "no commits yet"
        } else {
            "detached HEAD"
        };

        if args.dry_run {
            let plan = match (blocked, kind) {
                (true, _) => format!("BLOCKED ({blocked_why}) — ignore or fix, then re-run"),
                (false, Onboard::Create) => "would onboard → create -a (full topology)".into(),
                (false, Onboard::Repoint) => "would repoint (ADR-0018)".into(),
            };
            println!("  → {plan}\n");
            continue;
        }

        if blocked {
            println!("  ⚠ can't onboard as-is — {blocked_why}.");
        }
        match prompt_decision(blocked, *kind)? {
            Decision::Onboard => {
                match create_home(&cfg, repo, name, true, &repos, &alias, &audit) {
                    Ok(o) if !o.failed => tally.onboarded += 1,
                    Ok(_) => tally.failed += 1, // partial push; create_home already reported
                    Err(e) => {
                        eprintln!("  onboarding `{name}` failed: {e:#}");
                        tally.failed += 1;
                    }
                }
            }
            Decision::Repoint => {
                // ADR-0018 specifies the mechanics; not yet implemented. Leave the
                // repo untouched and ask again next run (treated as a skip).
                println!(
                    "  repoint isn't implemented yet (ADR-0018) — left as-is; \
                     choose ignore (n) if you want to stop the prompt."
                );
                tally.skipped += 1;
            }
            Decision::Ignore => {
                Config::append_ignore(name)?;
                println!(
                    "  ignored — recorded in {}",
                    Config::config_path().display()
                );
                tally.ignored += 1;
            }
            Decision::Skip => {
                println!("  skipped — will ask again next run.");
                tally.skipped += 1;
            }
            Decision::Quit => {
                println!("  quit — decisions so far are saved.");
                break;
            }
        }
        println!();
    }

    println!(
        "{} onboarded · {} ignored · {} skipped · {} failed",
        tally.onboarded, tally.ignored, tally.skipped, tally.failed
    );
    if tally.onboarded > 0 {
        if let Some(p) = audit.path() {
            println!("audit log: {}", p.display());
        }
    }
    if tally.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Print the per-repo context block: identity + size · branch · origin · last
/// commit, and a warning about uncommitted/untracked work that won't be backed
/// up (ADR-0017).
fn print_candidate(idx: usize, total: usize, name: &str, repo: &Path) -> Result<()> {
    let branch = git::current_branch(repo)?.unwrap_or_else(|| "(detached)".into());
    let size = dir_size_human(repo).unwrap_or_else(|| "?".into());
    let origin = match git::remote_url(repo, "origin")? {
        Some(url) => format!("{} origin", short_host(&url)),
        None => "no origin".into(),
    };
    let last = match git::last_commit_date(repo)? {
        Some(d) => format!("last commit {d}"),
        None => "no commits".into(),
    };
    println!("[{idx}/{total}]  {name}");
    println!("        {size} · {branch} · {origin} · {last}");

    let wt = git::working_tree(repo)?;
    let uncommitted = wt.staged + wt.unstaged + wt.conflicts;
    if uncommitted > 0 || wt.untracked > 0 {
        println!(
            "        {uncommitted} uncommitted, {} untracked (won't be backed up)",
            wt.untracked
        );
    }
    Ok(())
}

enum Decision {
    Onboard,
    Repoint,
    Ignore,
    Skip,
    Quit,
}

/// Prompt for one repo's decision, re-asking on invalid input. `y` (onboard) is
/// offered only when the repo isn't blocked and needs a fresh create; `r`
/// (repoint) replaces it for the backup-only sub-state.
fn prompt_decision(blocked: bool, kind: Onboard) -> Result<Decision> {
    let action = if blocked {
        None
    } else if kind == Onboard::Repoint {
        Some("repoint (r)")
    } else {
        Some("onboard (y)")
    };
    let prompt = match action {
        Some(a) => format!("  {a} / ignore (n) / skip (s) / quit (q) ? "),
        None => "  ignore (n) / skip (s) / quit (q) ? ".into(),
    };

    loop {
        print!("{prompt}");
        std::io::stdout().flush().ok();
        let mut input = String::new();
        // EOF (e.g. piped/closed stdin) reads as a graceful quit.
        if std::io::stdin().read_line(&mut input)? == 0 {
            return Ok(Decision::Quit);
        }
        match input.trim() {
            "y" | "Y" if action == Some("onboard (y)") => return Ok(Decision::Onboard),
            "r" | "R" if action == Some("repoint (r)") => return Ok(Decision::Repoint),
            "n" | "N" => return Ok(Decision::Ignore),
            "s" | "S" | "" => return Ok(Decision::Skip),
            "q" | "Q" => return Ok(Decision::Quit),
            other => println!("  (didn't understand `{other}`)"),
        }
    }
}

/// Best-effort human-readable size of the working copy (`du -sh`); `None` if the
/// tool isn't available, so the context line degrades gracefully.
fn dir_size_human(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("du")
        .args(["-sh", "--"])
        .arg(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.split_whitespace().next().map(|sz| sz.to_string())
}

/// Host portion of a remote URL for the context line: `git@github.com:o/r.git`
/// → `github.com`, `ssh://acer-lan/…` → `acer-lan`. Falls back to the raw URL.
fn short_host(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url)
        .trim_start_matches('/');
    let authority = rest.split(['/', ':']).next().unwrap_or(rest);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
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
