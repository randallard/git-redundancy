//! Server-side operations for the lifecycle commands (ADR-0013): create a bare
//! home, set its default branch, check existence — all over the ADR-0009 SSH
//! transport. Imperative shell; reuses inventory's alias resolution.
//!
//! These require the server: callers fail loudly (non-zero) when it's
//! unreachable rather than half-acting (ADR-0012 §5 / ADR-0013).

use crate::config::Config;
use crate::git::CmdOutcome;
use crate::inventory::{resolve_aliases, transport_remotes};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Same connect timeout as inventory — bounds a sleeping host.
const SSH_CONNECT_TIMEOUT: &str = "5";

/// Replication hooks vendored from the companion home-fleet project (ADR-0016).
/// `gr create` installs `POST_RECEIVE` into the primary home (immediate mirror to
/// the backup) and `PRE_RECEIVE` into the backup home (ff-only / no-delete guard).
pub const POST_RECEIVE_HOOK: &str = include_str!("hooks/post-receive");
pub const PRE_RECEIVE_HOOK: &str = include_str!("hooks/pre-receive");

/// The `ssh://<alias>/<root>/<name>.git` URL for a home (remotes + clone source).
pub fn home_url(alias: &str, root: &Path, name: &str) -> String {
    format!("ssh://{alias}{}/{name}.git", root.display())
}

/// Run one command on the server over `alias`, capturing success + stderr.
fn run(alias: &str, remote_cmd: &str) -> Result<CmdOutcome> {
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes"])
        .args(["-o", &format!("ConnectTimeout={SSH_CONNECT_TIMEOUT}")])
        .arg(alias)
        .arg(remote_cmd)
        .output()
        .with_context(|| format!("ssh {alias}"))?;
    Ok(CmdOutcome {
        success: out.status.success(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}

/// Run one command on the server over `alias`, feeding `stdin` to it (used to
/// write a hook file via `cat >`). Captures success + stderr.
fn run_with_stdin(alias: &str, remote_cmd: &str, stdin: &str) -> Result<CmdOutcome> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("ssh")
        .args(["-o", "BatchMode=yes"])
        .args(["-o", &format!("ConnectTimeout={SSH_CONNECT_TIMEOUT}")])
        .arg(alias)
        .arg(remote_cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("ssh {alias}"))?;
    child
        .stdin
        .take()
        .context("ssh stdin")?
        .write_all(stdin.as_bytes())
        .with_context(|| format!("writing to ssh {alias}"))?;
    let out = child
        .wait_with_output()
        .with_context(|| format!("ssh {alias}"))?;
    Ok(CmdOutcome {
        success: out.status.success(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}

/// First of `aliases` that answers a trivial command, i.e. the live transport.
/// `Err` when none connect (server unreachable).
fn first_live_alias(aliases: &[String], what: &str) -> Result<String> {
    if aliases.is_empty() {
        anyhow::bail!(
            "no SSH aliases to reach the {what} (set its `aliases` or wire a repo's remotes)"
        );
    }
    for alias in aliases {
        if run(alias, "true").map(|o| o.success).unwrap_or(false) {
            return Ok(alias.clone());
        }
    }
    anyhow::bail!("{what} unreachable over: {}", aliases.join(", "))
}

/// First alias that answers, i.e. the live primary transport.
pub fn pick_alias(cfg: &Config, repos: &[PathBuf]) -> Result<String> {
    first_live_alias(&resolve_aliases(cfg, repos), "server")
}

/// First live `[backup]` alias (explicit only — no per-repo backup remote to
/// derive from; ADR-0015).
pub fn pick_backup_alias(cfg: &Config) -> Result<String> {
    first_live_alias(&cfg.backup.aliases, "backup")
}

/// The `(remote-name, URL)` pairs to wire on a working copy for home `name`,
/// pairing each transport remote with an SSH alias positionally (ADR-0009:
/// `data-lan`↔`tenx-lan`, `data`↔`tenx-ts`). When aliases run short, the
/// remaining remotes reuse `fallback_alias` (the live one we just used).
pub fn remote_wiring(
    cfg: &Config,
    repos: &[PathBuf],
    root: &Path,
    name: &str,
    fallback_alias: &str,
) -> Vec<(String, String)> {
    let remotes = transport_remotes(cfg);
    let aliases = resolve_aliases(cfg, repos);
    remotes
        .iter()
        .enumerate()
        .map(|(i, remote)| {
            let alias = aliases.get(i).map(String::as_str).unwrap_or(fallback_alias);
            (remote.clone(), home_url(alias, root, name))
        })
        .collect()
}

/// Does `<root>/<name>.git` already exist on the server?
pub fn home_exists(alias: &str, root: &Path, name: &str) -> Result<bool> {
    let cmd = format!("test -d {}/{name}.git", root.display());
    Ok(run(alias, &cmd)?.success)
}

/// `git init --bare <root>/<name>.git` on the server.
pub fn init_bare(alias: &str, root: &Path, name: &str) -> Result<CmdOutcome> {
    let cmd = format!("git init --bare {}/{name}.git", root.display());
    run(alias, &cmd)
}

/// Point a fresh bare repo's `HEAD` at `branch`, so it doesn't read as empty
/// when the pushed branch isn't `master` (the documented `git init --bare`
/// gotcha).
pub fn set_head(alias: &str, root: &Path, name: &str, branch: &str) -> Result<CmdOutcome> {
    let cmd = format!(
        "git --git-dir={}/{name}.git symbolic-ref HEAD refs/heads/{branch}",
        root.display()
    );
    run(alias, &cmd)
}

/// Install (overwrite) a hook into `<root>/<name>.git/hooks/<hook>` with `body`,
/// made executable. Idempotent. Used by `gr create` for the primary `post-receive`
/// and the backup `pre-receive` (ADR-0016).
pub fn install_hook(
    alias: &str,
    root: &Path,
    name: &str,
    hook: &str,
    body: &str,
) -> Result<CmdOutcome> {
    let path = format!("{}/{name}.git/hooks/{hook}", root.display());
    let cmd = format!("cat > '{path}' && chmod 755 '{path}'");
    run_with_stdin(alias, &cmd, body)
}

/// Remove a hook from `<root>/<name>.git/hooks/<hook>` (`rm -f`, idempotent).
/// Used by `repoint` to drop a stale `post-receive` when a former primary home
/// is re-roled as a backup, so it can't mirror in the wrong direction (ADR-0018).
pub fn remove_hook(alias: &str, root: &Path, name: &str, hook: &str) -> Result<CmdOutcome> {
    let path = format!("{}/{name}.git/hooks/{hook}", root.display());
    run(alias, &format!("rm -f '{path}'"))
}

/// The shell to harden a backup home: fast-forward-only, no deletes (SI / ADR-0016,
/// matching the companion home-fleet's tenx-harden-homes.sh). Pure (testable).
fn harden_cmd(root: &Path, name: &str) -> String {
    let dir = format!("{}/{name}.git", root.display());
    format!(
        "git --git-dir={dir} config receive.denyNonFastForwards true && \
         git --git-dir={dir} config receive.denyDeletes true"
    )
}

/// Apply the fast-forward-only / no-delete config to a backup home (idempotent).
pub fn harden_home(alias: &str, root: &Path, name: &str) -> Result<CmdOutcome> {
    run(alias, &harden_cmd(root, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_url_shape() {
        assert_eq!(
            home_url("tenx-lan", Path::new("/data/git"), "omarchy-setup"),
            "ssh://tenx-lan/data/git/omarchy-setup.git"
        );
    }

    #[test]
    fn remote_wiring_pairs_transports_with_aliases_positionally() {
        let cfg = Config {
            transport: crate::config::Transport {
                order: vec!["data-lan".to_string(), "data".to_string()],
                ..Default::default()
            },
            server: crate::config::Server {
                aliases: vec!["tenx-lan".to_string(), "tenx-ts".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let wiring = remote_wiring(&cfg, &[], Path::new("/data/git"), "proj", "tenx-lan");
        assert_eq!(
            wiring,
            vec![
                (
                    "data-lan".to_string(),
                    "ssh://tenx-lan/data/git/proj.git".to_string()
                ),
                (
                    "data".to_string(),
                    "ssh://tenx-ts/data/git/proj.git".to_string()
                ),
            ]
        );
    }

    #[test]
    fn pick_alias_without_any_aliases_errors_before_ssh() {
        // No explicit aliases and no repos to derive from ⇒ bail, no ssh attempted.
        let cfg = Config::default();
        let err = pick_alias(&cfg, &[]).unwrap_err();
        assert!(err.to_string().contains("no SSH aliases"));
    }

    #[test]
    fn harden_cmd_sets_both_ff_guards() {
        let cmd = harden_cmd(Path::new("/data/git"), "proj");
        assert!(cmd.contains("--git-dir=/data/git/proj.git"));
        assert!(cmd.contains("receive.denyNonFastForwards true"));
        assert!(cmd.contains("receive.denyDeletes true"));
    }

    #[test]
    fn vendored_hooks_are_present_and_correct() {
        // Primary hook mirrors to the backup via the standing replication script.
        assert!(POST_RECEIVE_HOOK.starts_with("#!/usr/bin/env bash"));
        assert!(POST_RECEIVE_HOOK.contains("acer-mirror-one.sh"));
        // Backup hook enforces ff-only / no deletes.
        assert!(PRE_RECEIVE_HOOK.starts_with("#!/usr/bin/env bash"));
        assert!(PRE_RECEIVE_HOOK.contains("non-fast-forward"));
        assert!(PRE_RECEIVE_HOOK.contains("deletions are not allowed"));
    }
}
