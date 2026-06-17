//! `gr` — git-redundancy CLI. This increment implements `status`; `push` is
//! scaffolded and lands next (it needs the ADR-0009 SSH aliases in place).
#![forbid(unsafe_code)]

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
}

#[derive(clap::Args)]
struct StatusArgs {
    /// One row per local branch instead of just the current branch.
    #[arg(short = 'a', long)]
    all_branches: bool,
    /// Limit the table to a single remote.
    #[arg(long)]
    remote: Option<String>,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        // Default command (no subcommand) is `status`.
        None => run_status(&StatusArgs {
            all_branches: false,
            remote: None,
        }),
        Some(Command::Status(args)) => run_status(&args),
        Some(Command::Push(args)) => push::run_push(&args),
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

    println!("{}", render::table(&shown_remotes, &rows));
    Ok(())
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
