//! `gr` — git-redundancy CLI. `status` (home-aware, with a `<repo>` detail view,
//! ADR-0014) · `push` · `create` / `clone` / `sync` lifecycle verbs (ADR-0013) ·
//! `homes` (the ADR-0012 inventory, now also surfaced in `status`).
#![forbid(unsafe_code)]

mod lifecycle;
mod push;
mod render;
mod statusjson;

use anyhow::Result;
use clap::{Parser, Subcommand};
use git_redundancy_core::{BranchSync, SyncAction};
use git_redundancy_io::{config::Config, discovery::discover, git, server};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "gr",
    version,
    about = "git-redundancy: multi-repo status + push"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show a table of configured repos: working-tree state and per-remote sync.
    Status(StatusArgs),
    /// Push easy (fast-forward / new-branch), committed work home (skips behind/diverged, never forces).
    Push(PushArgs),
    /// Alias for `status` — the lifecycle is now a column there (ADR-0012/0014).
    Homes(HomesArgs),
    /// Create a bare home for the current repo, wire the remotes, and push (ADR-0013).
    Create(CreateArgs),
    /// Clone a home-only repo into a configured root and wire its remotes (ADR-0013).
    Clone(CloneArgs),
    /// Reconcile easy work both ways: push ahead, fast-forward behind (ADR-0013).
    Sync(SyncArgs),
    /// Walk un-redundant repos one at a time: onboard / ignore / skip / quit (ADR-0017).
    Onboard(OnboardArgs),
}

#[derive(clap::Args)]
struct StatusArgs {
    /// One repo (by home or directory name) → all-branches detail view (ADR-0014).
    repo: Option<String>,
    /// One row per local branch instead of just the current branch.
    #[arg(short = 'a', long)]
    all_branches: bool,
    /// Limit the table to a single remote.
    #[arg(long)]
    remote: Option<String>,
    /// Skip the server query; show the local view with lifecycle unknown (`?`).
    #[arg(long)]
    offline: bool,
    /// Emit machine-readable JSON instead of the table.
    #[arg(long)]
    json: bool,
    /// Disable colored output (also auto-disabled when not a TTY or NO_COLOR is set).
    #[arg(long)]
    no_color: bool,
}

#[derive(clap::Args)]
pub struct PushArgs {
    /// Push every local branch instead of just the current branch.
    #[arg(short = 'a', long)]
    pub all_branches: bool,
    /// Push to exactly this remote (disables transport failover).
    #[arg(long)]
    pub remote: Option<String>,
    /// Limit to these repos (by directory name); repeatable.
    #[arg(long = "only")]
    pub only: Vec<String>,
    /// Show what would be pushed without updating any remote.
    #[arg(long)]
    pub dry_run: bool,
    /// Also push annotated tags reachable from pushed commits (--follow-tags).
    #[arg(long)]
    pub tags: bool,
}

#[derive(clap::Args)]
struct HomesArgs {
    /// Skip the network: show local repos only (no server query).
    #[arg(long)]
    offline: bool,
}

#[derive(clap::Args)]
pub struct CreateArgs {
    /// Home name (default: current directory name).
    pub name: Option<String>,
    /// Push every local branch, not just the current one.
    #[arg(short = 'a', long)]
    pub all_branches: bool,
}

#[derive(clap::Args)]
pub struct OnboardArgs {
    /// Preview the walk: show each candidate and what would happen, prompt for
    /// nothing, change nothing (and don't audit).
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(clap::Args)]
pub struct CloneArgs {
    /// Home name to clone (`<root>/<name>.git` on the server).
    pub name: String,
    /// Target directory (must be inside a configured root; default `<roots[0]>/<name>`).
    pub dir: Option<std::path::PathBuf>,
}

#[derive(clap::Args)]
pub struct SyncArgs {
    /// Reconcile every local branch instead of just the current one.
    #[arg(short = 'a', long)]
    pub all_branches: bool,
    /// Confirm each effecting action (push / fast-forward) before it runs.
    #[arg(short = 'i', long)]
    pub interactive: bool,
    /// Show what would happen without changing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Limit to these repos (by directory name); positional, repeatable.
    pub repos: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        // Default command (no subcommand) is `status`.
        None => run_status(&StatusArgs {
            repo: None,
            all_branches: false,
            remote: None,
            offline: false,
            json: false,
            no_color: false,
        }),
        Some(Command::Status(args)) => run_status(&args),
        Some(Command::Push(args)) => push::run_push(&args),
        // `homes` is a thin alias for the fleet `status` view (ADR-0014).
        Some(Command::Homes(args)) => run_status(&StatusArgs {
            repo: None,
            all_branches: false,
            remote: None,
            offline: args.offline,
            json: false,
            no_color: false,
        }),
        Some(Command::Create(args)) => lifecycle::run_create(&args),
        Some(Command::Clone(args)) => lifecycle::run_clone(&args),
        Some(Command::Sync(args)) => lifecycle::run_sync(&args),
        Some(Command::Onboard(args)) => lifecycle::run_onboard(&args),
    }
}

fn file_name_string(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Survey the home inventory for lifecycle, honoring `--offline` (skip the
/// network). Returns the survey plus whether the home side is actually known.
fn status_survey(cfg: &Config, offline: bool) -> (git_redundancy_io::Survey, bool) {
    if offline || !cfg.server_enabled() {
        let mut local_cfg = cfg.clone();
        local_cfg.server.root.clear();
        if offline {
            local_cfg.backup.root.clear(); // --offline skips the backup query too
        }
        (git_redundancy_io::survey(&local_cfg), false)
    } else {
        let s = git_redundancy_io::survey(cfg);
        let known = s.reachable;
        (s, known)
    }
}

fn run_status(args: &StatusArgs) -> Result<()> {
    let cfg = Config::load()?;
    if cfg.is_empty() {
        println!(
            "No repos configured. Add roots/repos to {}.",
            Config::config_path().display()
        );
        return Ok(());
    }

    let repos = discover(&cfg);
    let path_by_dir: BTreeMap<String, PathBuf> = repos
        .iter()
        .map(|p| (file_name_string(p), p.clone()))
        .collect();
    let (survey, home_known) = status_survey(&cfg, args.offline);

    if let Some(target) = &args.repo {
        return run_status_detail(
            &cfg,
            &repos,
            &path_by_dir,
            &survey,
            target,
            args,
            home_known,
        );
    }

    let shown = shown_remotes(args, &cfg, &repos)?;

    if survey.presences.is_empty() {
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&statusjson::fleet(&shown, &[]))?
            );
        } else {
            println!("No repos found under the configured roots/repos.");
        }
        return Ok(());
    }

    let show_backup = survey.backup.is_some();
    let mut rows = Vec::new();
    for p in &survey.presences {
        // A deliberately-ignored repo (ADR-0017) stays visible but reads `ignored`
        // and drops the `+N⚠` nag — the operator already decided not to back it up.
        let ignored = cfg.is_ignored(&p.home_name)
            || p.local_dir.as_deref().is_some_and(|d| cfg.is_ignored(d));
        let life = if ignored {
            "ignored".to_string()
        } else if home_known {
            p.lifecycle.label().to_string()
        } else {
            "?".to_string()
        };
        let backup = backup_label(&survey.backup, &p.home_name);
        match p.local_dir.as_ref().and_then(|d| path_by_dir.get(d)) {
            // Local repos display their on-disk directory name; the home name is
            // the internal identity (and the `gr status <name>` detail header).
            Some(repo) => build_local_rows(
                repo,
                &file_name_string(repo),
                &life,
                &backup,
                ignored,
                &shown,
                args,
                &mut rows,
            )?,
            None => {
                // home-only repo (only present when the home side is known).
                let mut row = render::Row::new(p.home_name.clone(), "(home)".into(), false);
                row.lifecycle = life;
                row.backup = backup;
                row.remote_cells = shown.iter().map(|_| None).collect();
                rows.push(row);
            }
        }
    }

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&statusjson::fleet(&shown, &rows))?
        );
        return Ok(());
    }

    if !home_known && !args.offline {
        if cfg.server_enabled() {
            println!("(server unreachable — lifecycle shown as `?`, home-only repos hidden)\n");
        } else {
            println!(
                "(no [server] configured — lifecycle hidden; add a [server] block with `root = \"/data/git\"` to {} to enable it)\n",
                Config::config_path().display()
            );
        }
    }
    println!(
        "{}",
        render::table(&shown, &rows, color_enabled(args.no_color), show_backup)
    );
    Ok(())
}

/// Per-repo backup-presence label for the `Bkp` column: `ok` if the repo's home
/// is on the backup server, `miss` if not, `?` if the backup is unreachable, and
/// `""` (no cell) when no `[backup]` is configured.
fn backup_label(backup: &Option<git_redundancy_io::BackupState>, home_name: &str) -> String {
    match backup {
        None => String::new(),
        Some(b) if !b.reachable => "?".to_string(),
        Some(b) => {
            if b.homes.iter().any(|h| h == home_name) {
                "ok".to_string()
            } else {
                "miss".to_string()
            }
        }
    }
}

/// Build the fleet rows for one local repo: branch rows + lifecycle on the first
/// row + the `+N⚠` indicator on the current row (default view only).
#[allow(clippy::too_many_arguments)]
fn build_local_rows(
    repo: &Path,
    display: &str,
    life: &str,
    backup: &str,
    ignored: bool,
    shown: &[String],
    args: &StatusArgs,
    rows: &mut Vec<render::Row>,
) -> Result<()> {
    let current = git::current_branch(repo)?;
    let wt = git::working_tree(repo)?;
    let repo_remotes: BTreeSet<String> = git::remotes(repo)?.into_iter().collect();
    let branches: Vec<String> = if args.all_branches {
        git::local_branches(repo)?
    } else {
        current.clone().into_iter().collect()
    };

    if branches.is_empty() {
        let mut row = render::Row::new(
            display.to_string(),
            current.clone().unwrap_or_else(|| "(detached)".into()),
            true,
        );
        row.lifecycle = life.to_string();
        row.backup = backup.to_string();
        row.wt = Some(wt);
        row.remote_cells = shown.iter().map(|_| None).collect();
        rows.push(row);
        return Ok(());
    }

    // Primary remote for the "others" count: the first shown remote present.
    let primary = shown.iter().find(|r| repo_remotes.contains(*r)).cloned();
    let others = if args.all_branches || ignored {
        None // -a: every branch is a row already; ignored: the nag is suppressed.
    } else {
        others_needing_attention(repo, &current, primary.as_deref())?
    };

    for (i, branch) in branches.iter().enumerate() {
        let is_current = current.as_deref() == Some(branch.as_str());
        let cells = shown
            .iter()
            .map(|r| sync_cell(repo, branch, r, &repo_remotes))
            .collect::<Result<Vec<_>>>()?;
        let mut row = render::Row::new(
            if i == 0 {
                display.to_string()
            } else {
                String::new()
            },
            branch.clone(),
            is_current,
        );
        if i == 0 {
            row.lifecycle = life.to_string();
            row.backup = backup.to_string();
        }
        row.wt = if is_current { Some(wt) } else { None };
        row.remote_cells = cells;
        if is_current {
            row.others = others;
        }
        rows.push(row);
    }
    Ok(())
}

/// Count branches *other* than `current` that aren't up-to-date with the primary
/// home remote — the `+N⚠` hint so a clean current branch can't hide drift.
fn others_needing_attention(
    repo: &Path,
    current: &Option<String>,
    primary: Option<&str>,
) -> Result<Option<u32>> {
    let Some(remote) = primary else {
        return Ok(None);
    };
    let mut n = 0;
    for b in git::local_branches(repo)? {
        if current.as_deref() == Some(b.as_str()) {
            continue;
        }
        if !matches!(git::branch_sync(repo, &b, remote)?, BranchSync::UpToDate) {
            n += 1;
        }
    }
    Ok(Some(n))
}

/// `gr status <repo>` — one repo, every branch, with the action `sync` would
/// take. Works for a local repo or a home-only one (branches via `ls-remote`).
fn run_status_detail(
    cfg: &Config,
    repos: &[PathBuf],
    path_by_dir: &BTreeMap<String, PathBuf>,
    survey: &git_redundancy_io::Survey,
    target: &str,
    args: &StatusArgs,
    home_known: bool,
) -> Result<()> {
    let Some(p) = survey
        .presences
        .iter()
        .find(|p| p.home_name == target || p.local_dir.as_deref() == Some(target))
    else {
        if args.json {
            println!("null");
        } else {
            println!("No repo named `{target}`. Run `gr status` to list them.");
        }
        return Ok(());
    };

    let shown = shown_remotes(args, cfg, repos)?;
    let color = color_enabled(args.no_color);
    let local = p.local_dir.as_ref().and_then(|d| path_by_dir.get(d));
    let mut local_branches: BTreeSet<String> = BTreeSet::new();
    let mut rows = Vec::new();

    if let Some(repo) = local {
        let current = git::current_branch(repo)?;
        let wt = git::working_tree(repo)?;
        let repo_remotes: BTreeSet<String> = git::remotes(repo)?.into_iter().collect();
        let primary = shown.iter().find(|r| repo_remotes.contains(*r)).cloned();
        for b in git::local_branches(repo)? {
            local_branches.insert(b.clone());
            let is_current = current.as_deref() == Some(b.as_str());
            let cells = shown
                .iter()
                .map(|r| sync_cell(repo, &b, r, &repo_remotes))
                .collect::<Result<Vec<_>>>()?;
            let action = match &primary {
                Some(r) => {
                    let sync = git::branch_sync(repo, &b, r)?;
                    let tree_clean = if is_current { wt.is_clean() } else { true };
                    Some(action_label(sync, SyncAction::plan(sync, tree_clean)))
                }
                None => None,
            };
            let mut row = render::Row::new(String::new(), b.clone(), is_current);
            row.wt = if is_current { Some(wt) } else { None };
            row.remote_cells = cells;
            row.action = action;
            rows.push(row);
        }
    }

    // Home-only branches (present on the home, not local) — one `ls-remote`.
    if home_known && cfg.server_enabled() {
        if let Ok(alias) = server::pick_alias(cfg, repos) {
            let url = server::home_url(&alias, &cfg.server.root, &p.home_name);
            if let Ok(home_branches) = git::ls_remote_heads(&url) {
                for b in home_branches {
                    if !local_branches.contains(&b) {
                        let mut row = render::Row::new(String::new(), b, false);
                        row.remote_cells = shown.iter().map(|_| None).collect();
                        row.action = Some(if local.is_some() {
                            "fetch".into()
                        } else {
                            "clone".into()
                        });
                        rows.push(row);
                    }
                }
            }
        }
    }

    let life = if home_known { p.lifecycle.label() } else { "?" };
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&statusjson::detail(&p.home_name, life, &shown, &rows))?
        );
        return Ok(());
    }
    println!("{}  [{}]", p.home_name, life);
    println!("{}", render::detail_table(&shown, &rows, color));
    Ok(())
}

/// The detail view's `sync` column text for a branch.
fn action_label(sync: BranchSync, action: SyncAction) -> String {
    match action {
        SyncAction::UpToDate => "ok".into(),
        SyncAction::Push => match sync {
            BranchSync::NoRemoteBranch => "push (new)".into(),
            BranchSync::Ahead(n) => format!("push ↑{n}"),
            _ => "push".into(),
        },
        SyncAction::FastForward(n) => format!("ff ↓{n}"),
        SyncAction::BlockedDirty(n) => format!("↓{n} dirty"),
        SyncAction::Report => match sync {
            BranchSync::Diverged { conflict: true, .. } => "CONFLICT".into(),
            _ => "diverged".into(),
        },
    }
}

/// Color is on for a TTY unless `--no-color` or `NO_COLOR` is set; `CLICOLOR_FORCE`
/// forces it on (handy for piping into a pager).
fn color_enabled(no_color: bool) -> bool {
    use std::io::IsTerminal;
    if no_color {
        return false;
    }
    if let Some(v) = std::env::var_os("CLICOLOR_FORCE") {
        if v.to_string_lossy() != "0" {
            return true;
        }
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// Compute one remote cell for a (repo, branch, remote).
fn sync_cell(
    repo: &Path,
    branch: &str,
    remote: &str,
    repo_remotes: &BTreeSet<String>,
) -> Result<Option<BranchSync>> {
    if !repo_remotes.contains(remote) {
        return Ok(None);
    }
    Ok(Some(git::branch_sync(repo, branch, remote)?))
}

/// Decide which remote columns to render.
fn shown_remotes(
    args: &StatusArgs,
    cfg: &Config,
    repos: &[std::path::PathBuf],
) -> Result<Vec<String>> {
    if let Some(r) = &args.remote {
        return Ok(vec![r.clone()]);
    }
    if !cfg.default_remotes.is_empty() {
        return Ok(cfg.default_remotes.clone());
    }
    // Fall back to the union of every repo's remotes.
    let mut set = BTreeSet::new();
    for repo in repos {
        for r in git::remotes(repo)? {
            set.insert(r);
        }
    }
    Ok(set.into_iter().collect())
}
