//! Bare-`gr` review tail (ADR-0022): after the status table prints, offer to
//! cycle through dirty repos and stage/commit them interactively. Never runs
//! for `gr status`/`gr homes`, which stay pure, scriptable, read-only views.
//!
//! Honors ADR-0006 ("never auto-commit"): `gr` never composes a commit or
//! stages a file on its own here either — every `add` and every `commit`
//! happens because the operator answered a per-file prompt and typed an
//! actual commit message.

use crate::color_enabled;
use anyhow::Result;
use git_redundancy_core::{EntryKind, StatusEntry};
use git_redundancy_io::{config::Config, discovery::discover, git};
use std::io::Write;
use std::path::{Path, PathBuf};

struct DirtyRepo {
    name: String,
    path: PathBuf,
    branch: String,
    staged: u32,
    unstaged: u32,
    untracked: u32,
    conflicts: u32,
}

/// Scan the fleet for dirty repos; if any, offer the interactive stage/review
/// walk. A clean fleet (the common case) prints nothing beyond the status
/// table already shown by the caller.
pub fn maybe_prompt() -> Result<()> {
    let cfg = Config::load()?;
    if cfg.is_empty() {
        return Ok(());
    }

    let mut dirty = Vec::new();
    for repo in discover(&cfg) {
        let name = repo_name(&repo);
        if cfg.is_ignored(&name) || cfg.is_ignored(&repo.to_string_lossy()) {
            continue;
        }
        let wt = git::working_tree(&repo)?;
        if wt.is_clean() {
            continue;
        }
        let branch = git::current_branch(&repo)?.unwrap_or_else(|| "(detached)".into());
        dirty.push(DirtyRepo {
            name,
            path: repo,
            branch,
            staged: wt.staged,
            unstaged: wt.unstaged,
            untracked: wt.untracked,
            conflicts: wt.conflicts,
        });
    }
    if dirty.is_empty() {
        return Ok(());
    }

    println!("\n{} repo(s) have uncommitted work.", dirty.len());
    print!("[Enter] quit  \u{b7}  s) stage & review \u{203a} ");
    std::io::stdout().flush().ok();
    let mut input = String::new();
    // EOF (piped/closed stdin) reads as a graceful quit, same as `gr onboard`.
    if std::io::stdin().read_line(&mut input)? == 0 {
        return Ok(());
    }
    if !matches!(input.trim(), "s" | "S" | "stage") {
        return Ok(());
    }

    let color = color_enabled(false);
    let total = dirty.len();
    for repo in &dirty {
        review_repo(repo, color)?;
    }
    println!(
        "\n{total} repo(s) reviewed. Run `gr push` or `gr sync` to back up anything you committed."
    );
    Ok(())
}

/// Walk one dirty repo's changed paths, offering to stage each, then (if
/// anything ended up staged) prompt for a commit message.
fn review_repo(repo: &DirtyRepo, color: bool) -> Result<()> {
    let mut parts = Vec::new();
    if repo.staged > 0 {
        parts.push(format!("{} staged", repo.staged));
    }
    if repo.unstaged > 0 {
        parts.push(format!("{} unstaged", repo.unstaged));
    }
    if repo.conflicts > 0 {
        parts.push(format!("{} conflicts", repo.conflicts));
    }
    if repo.untracked > 0 {
        parts.push(format!("{} untracked", repo.untracked));
    }
    println!(
        "\n{} ({}) \u{2014} {}",
        repo.name,
        repo.branch,
        parts.join(", ")
    );

    for entry in git::status_entries(&repo.path)? {
        match entry.kind {
            EntryKind::Conflict => {
                println!(
                    "  \u{26a0} {}: merge conflict \u{2014} resolve manually",
                    entry.path
                );
            }
            EntryKind::StagedOnly => {
                println!("  {}: already staged", entry.path);
            }
            EntryKind::Untracked | EntryKind::Modified => {
                stage_file(&repo.path, &entry, color)?;
            }
        }
    }

    let wt = git::working_tree(&repo.path)?;
    if wt.staged == 0 {
        return Ok(());
    }
    print!("  commit message (empty to skip): ");
    std::io::stdout().flush().ok();
    let mut msg = String::new();
    std::io::stdin().read_line(&mut msg)?;
    let msg = msg.trim();
    if msg.is_empty() {
        println!("  left staged \u{2014} not committed");
        return Ok(());
    }
    let out = git::commit(&repo.path, msg)?;
    if out.success {
        println!("  committed {} {}", repo.name, repo.branch);
    } else {
        eprintln!("  commit failed: {}", first_line(&out.stderr));
    }
    Ok(())
}

/// Show `entry`'s diff, then prompt to stage it — `y` adds, `e` opens
/// `$EDITOR` on the file and re-shows the diff, anything else (including EOF)
/// leaves it unstaged.
fn stage_file(repo: &Path, entry: &StatusEntry, color: bool) -> Result<()> {
    loop {
        let diff = if entry.kind == EntryKind::Untracked {
            git::diff_untracked(repo, &entry.path, color)?
        } else {
            git::diff_unstaged(repo, &entry.path, color)?
        };
        if diff.is_empty() {
            println!(
                "  {} (no textual diff \u{2014} binary or empty file)",
                entry.path
            );
        } else {
            print!("{diff}");
        }
        print!("  stage {}? [y/N/e] ", entry.path);
        std::io::stdout().flush().ok();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input)? == 0 {
            return Ok(());
        }
        match input.trim() {
            "y" | "Y" => {
                let out = git::add_file(repo, &entry.path)?;
                if !out.success {
                    eprintln!("  add failed: {}", first_line(&out.stderr));
                }
                return Ok(());
            }
            "e" | "E" => {
                open_editor(repo, &entry.path)?;
                // loop back around: re-diff and re-prompt after editing
            }
            _ => return Ok(()),
        }
    }
}

/// Open `$EDITOR` (default `vi`) on `rel`, via a shell so a multi-word editor
/// command (e.g. `EDITOR="code --wait"`) works; the path is passed as `$1`
/// rather than interpolated, so it's never shell-injected regardless of its
/// contents.
fn open_editor(repo: &Path, rel: &str) -> Result<()> {
    let full = repo.join(rel);
    std::process::Command::new("sh")
        .arg("-c")
        .arg(r#"${EDITOR:-vi} -- "$1""#)
        .arg("sh") // becomes $0 inside the shell, leaving $1 == the path
        .arg(&full)
        .status()?;
    Ok(())
}

fn repo_name(repo: &Path) -> String {
    repo.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.display().to_string())
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}
