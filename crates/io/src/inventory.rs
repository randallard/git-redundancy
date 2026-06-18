//! Home inventory (ADR-0012): list the bare repos on the server and join them
//! with local working copies into a lifecycle view. Imperative shell — SSH and
//! local `git` reads; the identity/join rules are pure in `core::presence`.
//!
//! Reads degrade: when the server is unreachable, `survey` returns the
//! local-only view with `reachable = false` rather than failing (ADR-0012 §5).

use crate::config::Config;
use crate::{discovery, git};
use anyhow::{Context, Result};
use git_redundancy_core::presence::{home_name_from_url, join_presences, LocalRepo, RepoPresence};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Seconds to wait for the inventory SSH connection before treating the server
/// as unreachable (keeps a sleeping tenx from hanging a `status`).
const SSH_CONNECT_TIMEOUT: &str = "5";

/// Result of surveying both sides. `reachable = false` means the server's home
/// listing could not be obtained (unconfigured or unreachable), so `presences`
/// reflects the local-only view.
pub struct Survey {
    pub presences: Vec<RepoPresence>,
    pub reachable: bool,
}

/// The remotes that name the transport paths to the home, in preference order
/// (`transport.order`, falling back to `default_remotes`).
fn transport_remotes(cfg: &Config) -> Vec<String> {
    if cfg.transport.order.is_empty() {
        cfg.default_remotes.clone()
    } else {
        cfg.transport.order.clone()
    }
}

/// Extract the SSH alias (host) from a remote URL: `ssh://tenx-lan/data/...` →
/// `tenx-lan`, `ssh://user@tenx-ts/...` → `tenx-ts`. `None` for non-ssh URLs.
pub fn ssh_alias_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("ssh://")?;
    let authority = rest.split(['/', ':']).next()?;
    let host = authority.rsplit('@').next().unwrap_or(authority);
    (!host.is_empty()).then(|| host.to_string())
}

/// Parse `ls -d <root>/*.git` output into home names (basename minus `.git`).
/// Skips a no-match line (the glob echoed back verbatim has no `.git` basename).
fn parse_home_list(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let path = line.trim().trim_end_matches('/');
            let base = path.rsplit('/').next().unwrap_or(path);
            base.strip_suffix(".git")
                // Drop empties and an unexpanded glob (`*.git` echoed on no match).
                .filter(|n| !n.is_empty() && !n.contains('*'))
                .map(str::to_string)
        })
        .collect()
}

/// SSH aliases to try, in order: explicit `[server].aliases`, else derived from
/// the transport remotes' URLs across discovered repos (de-duplicated).
fn resolve_aliases(cfg: &Config, repos: &[PathBuf]) -> Vec<String> {
    if !cfg.server.aliases.is_empty() {
        return cfg.server.aliases.clone();
    }
    let remotes = transport_remotes(cfg);
    let mut out: Vec<String> = Vec::new();
    for repo in repos {
        for remote in &remotes {
            if let Ok(Some(url)) = git::remote_url(repo, remote) {
                if let Some(alias) = ssh_alias_of(&url) {
                    if !out.contains(&alias) {
                        out.push(alias);
                    }
                }
            }
        }
    }
    out
}

/// List bare-repo home names under `root` on the server, trying `aliases` in
/// order and returning the first that connects. `Err` only when none connect.
fn list_homes(aliases: &[String], root: &Path) -> Result<Vec<String>> {
    if aliases.is_empty() {
        anyhow::bail!(
            "no SSH aliases to reach the server (set [server].aliases or wire a repo's remotes)"
        );
    }
    let listing = format!("ls -d {}/*.git 2>/dev/null", root.display());
    let mut last_err = anyhow::anyhow!("no alias attempted");
    for alias in aliases {
        let out = Command::new("ssh")
            .args(["-o", "BatchMode=yes"])
            .args(["-o", &format!("ConnectTimeout={SSH_CONNECT_TIMEOUT}")])
            .arg(alias)
            .arg(&listing)
            .output()
            .with_context(|| format!("running ssh {alias}"))?;
        if out.status.success() {
            return Ok(parse_home_list(&String::from_utf8_lossy(&out.stdout)));
        }
        last_err = anyhow::anyhow!(
            "ssh {alias}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Err(last_err)
}

/// Build the local side: each discovered repo's directory name plus the home
/// name recovered from the first present transport remote's URL.
fn local_repos(cfg: &Config, repos: &[PathBuf]) -> Vec<LocalRepo> {
    let remotes = transport_remotes(cfg);
    repos
        .iter()
        .map(|repo| {
            let dir_name = repo
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let home_name = remotes.iter().find_map(|remote| {
                git::remote_url(repo, remote)
                    .ok()
                    .flatten()
                    .and_then(|url| home_name_from_url(&url))
            });
            LocalRepo {
                dir_name,
                home_name,
            }
        })
        .collect()
}

/// Survey both sides into a lifecycle view (ADR-0012). The server listing is
/// queried over SSH; if it is unconfigured or unreachable the result degrades
/// to the local-only view with `reachable = false`.
pub fn survey(cfg: &Config) -> Survey {
    let repos = discovery::discover(cfg);
    let locals = local_repos(cfg, &repos);

    if !cfg.server_enabled() {
        return Survey {
            presences: join_presences(&locals, &[]),
            reachable: false,
        };
    }

    let aliases = resolve_aliases(cfg, &repos);
    match list_homes(&aliases, &cfg.server.root) {
        Ok(homes) => Survey {
            presences: join_presences(&locals, &homes),
            reachable: true,
        },
        Err(_) => Survey {
            presences: join_presences(&locals, &[]),
            reachable: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_alias_extraction() {
        assert_eq!(
            ssh_alias_of("ssh://tenx-lan/data/git/x.git").as_deref(),
            Some("tenx-lan")
        );
        assert_eq!(
            ssh_alias_of("ssh://randallard@tenx-ts/data/git/x.git").as_deref(),
            Some("tenx-ts")
        );
        // Non-ssh (local path / scp-like) → no alias to derive.
        assert_eq!(ssh_alias_of("/data/git/x.git"), None);
        assert_eq!(ssh_alias_of("git@github.com:o/r.git"), None);
    }

    #[test]
    fn parse_ls_output_to_home_names() {
        let stdout = "/data/git/cmecf_inside.git\n/data/git/omarchy-setup.git/\n";
        assert_eq!(
            parse_home_list(stdout),
            vec!["cmecf_inside".to_string(), "omarchy-setup".to_string()]
        );
        // A glob that matched nothing (echoed verbatim) yields no names.
        assert!(parse_home_list("/data/git/*.git\n").is_empty());
        assert!(parse_home_list("").is_empty());
    }

    #[test]
    fn transport_remotes_prefers_order_then_default() {
        // No transport.order → fall back to default_remotes.
        let cfg = Config {
            default_remotes: vec!["data".to_string()],
            ..Default::default()
        };
        assert_eq!(transport_remotes(&cfg), vec!["data".to_string()]);
        // transport.order wins when present.
        let cfg = Config {
            default_remotes: vec!["data".to_string()],
            transport: crate::config::Transport {
                order: vec!["data-lan".to_string(), "data".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            transport_remotes(&cfg),
            vec!["data-lan".to_string(), "data".to_string()]
        );
    }

    #[test]
    fn resolve_aliases_uses_explicit_server_aliases_verbatim() {
        // Explicit aliases short-circuit derivation, so no repos are needed.
        let cfg = Config {
            server: crate::config::Server {
                aliases: vec!["tenx-lan".to_string(), "tenx-ts".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            resolve_aliases(&cfg, &[]),
            vec!["tenx-lan".to_string(), "tenx-ts".to_string()]
        );
    }

    #[test]
    fn list_homes_without_aliases_errors_before_touching_the_network() {
        let err = list_homes(&[], Path::new("/data/git")).unwrap_err();
        assert!(err.to_string().contains("no SSH aliases"));
    }

    #[test]
    fn survey_without_server_is_local_only_and_unreachable() {
        // Empty roots ⇒ no repos discovered; server disabled ⇒ no network.
        let cfg = Config::default();
        let survey = survey(&cfg);
        assert!(!survey.reachable);
        assert!(survey.presences.is_empty());
    }
}
