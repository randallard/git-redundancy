//! `gr` — git-redundancy CLI. Implements `status` and `push`; `homes` surfaces
//! the ADR-0012 home inventory (folded into `status` by ADR-0014). The `create` /
//! `clone` / `sync` lifecycle verbs (ADR-0013) land next.
#![forbid(unsafe_code)]

mod lifecycle;
mod push;
mod render;

use anyhow::Result;
use clap::{Parser, Subcommand};
use git_redundancy_core::BranchSync;
use git_redundancy_io::{config::Config, discovery::discover, git};
use std::collections::BTreeSet;
use std::path::Path;

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
    /// Push easy (fast-forward), committed work home. (Not yet implemented.)
    Push(PushArgs),
    /// List the bare "home" repos on the server and each repo's lifecycle (ADR-0012).
    Homes(HomesArgs),
    /// Create a bare home for the current repo, wire the remotes, and push (ADR-0013).
    Create(CreateArgs),
    /// Clone a home-only repo into a configured root and wire its remotes (ADR-0013).
    Clone(CloneArgs),
    /// Reconcile easy work both ways: push ahead, fast-forward behind (ADR-0013).
    Sync(SyncArgs),
}

#[derive(clap::Args)]
struct StatusArgs {
    /// One row per local branch instead of just the current branch.
    #[arg(short = 'a', long)]
    all_branches: bool,
    /// Limit the table to a single remote.
    #[arg(long)]
    remote: Option<String>,
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
            all_branches: false,
            remote: None,
            no_color: false,
        }),
        Some(Command::Status(args)) => run_status(&args),
        Some(Command::Push(args)) => push::run_push(&args),
        Some(Command::Homes(args)) => run_homes(&args),
        Some(Command::Create(args)) => lifecycle::run_create(&args),
        Some(Command::Clone(args)) => lifecycle::run_clone(&args),
        Some(Command::Sync(args)) => lifecycle::run_sync(&args),
    }
}

/// `gr homes` — the ADR-0012 home inventory: every repo's lifecycle
/// (`local-only` / `home-only` / `linked`), joining local working copies with
/// the bare homes on the server. This is the foundation that ADR-0014 folds into
/// `gr status`; for now it's a standalone view of the two-sided picture.
fn run_homes(args: &HomesArgs) -> Result<()> {
    let cfg = Config::load()?;
    if !cfg.server_enabled() {
        println!(
            "No [server] configured in {}.\n\nAdd one to enable home inventory, e.g.:\n\n  [server]\n  root = \"/data/git\"\n  # aliases = [\"tenx-lan\", \"tenx-ts\"]   # else derived from your repos' remotes",
            Config::config_path().display()
        );
        return Ok(());
    }

    // `--offline` disables the server query by surveying with no server root,
    // which yields the local-only view (every repo as local-only / unmatched).
    let survey = if args.offline {
        let mut local_cfg = cfg.clone();
        local_cfg.server.root.clear();
        git_redundancy_io::survey(&local_cfg)
    } else {
        git_redundancy_io::survey(&cfg)
    };

    if survey.presences.is_empty() {
        println!("No repos found (configured roots are empty or unreadable).");
        return Ok(());
    }
    if !args.offline && !survey.reachable {
        println!("(server unreachable — showing local repos only)\n");
    }

    for p in &survey.presences {
        let local = p.local_dir.as_deref().unwrap_or("—");
        println!(
            "  {:<24} {:<11} local: {}",
            p.home_name,
            p.lifecycle.label(),
            local
        );
    }
    Ok(())
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
    if repos.is_empty() {
        println!("No repos found under the configured roots/repos.");
        return Ok(());
    }

    // Which remote columns to show.
    let shown_remotes = shown_remotes(args, &cfg, &repos)?;

    let mut rows = Vec::new();
    for repo in &repos {
        let name = repo
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo.display().to_string());
        let current = git::current_branch(repo)?;
        let wt = git::working_tree(repo)?;
        let repo_remotes: BTreeSet<String> = git::remotes(repo)?.into_iter().collect();

        let branches: Vec<String> = if args.all_branches {
            git::local_branches(repo)?
        } else {
            current.clone().into_iter().collect()
        };

        if branches.is_empty() {
            // Detached HEAD or no branches: still show a row.
            rows.push(render::Row {
                repo: name.clone(),
                branch: current.clone().unwrap_or_else(|| "(detached)".into()),
                is_current: true,
                wt: Some(wt),
                remote_cells: shown_remotes.iter().map(|_| None).collect(),
            });
            continue;
        }

        for (i, branch) in branches.iter().enumerate() {
            let is_current = current.as_deref() == Some(branch.as_str());
            let cells = shown_remotes
                .iter()
                .map(|remote| sync_cell(repo, branch, remote, &repo_remotes))
                .collect::<Result<Vec<_>>>()?;
            rows.push(render::Row {
                repo: if i == 0 { name.clone() } else { String::new() },
                branch: branch.clone(),
                is_current,
                wt: if is_current { Some(wt) } else { None },
                remote_cells: cells,
            });
        }
    }

    println!(
        "{}",
        render::table(&shown_remotes, &rows, color_enabled(args.no_color))
    );
    Ok(())
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
