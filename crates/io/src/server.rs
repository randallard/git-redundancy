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

/// First alias that answers a trivial command, i.e. the live transport. `Err`
/// when none connect (server unreachable).
pub fn pick_alias(cfg: &Config, repos: &[PathBuf]) -> Result<String> {
    let aliases = resolve_aliases(cfg, repos);
    if aliases.is_empty() {
        anyhow::bail!(
            "no SSH aliases to reach the server (set [server].aliases or wire a repo's remotes)"
        );
    }
    for alias in &aliases {
        if run(alias, "true").map(|o| o.success).unwrap_or(false) {
            return Ok(alias.clone());
        }
    }
    anyhow::bail!("server unreachable over: {}", aliases.join(", "))
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
}
