//! Config loading. **Config-first** (PROGRESS §5): git-redundancy acts only on
//! what the config declares — no implicit global scan, no built-in default path.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    /// Roots to discover repos *within* (each immediate child holding a `.git`).
    pub roots: Vec<PathBuf>,
    /// Explicit repo paths to include in addition to whatever the roots find.
    pub repos: Vec<PathBuf>,
    /// Paths to exclude even if found under a root.
    pub exclude: Vec<PathBuf>,
    /// Remotes to show as columns / push to, in order. Empty = use each repo's own remotes.
    pub default_remotes: Vec<String>,
    /// Push transport behavior (ADR-0009).
    pub transport: Transport,
    /// Server-side bare-repo home inventory (ADR-0012).
    pub server: Server,
    /// Optional second "backup" home server (a replicated mirror of `server`).
    /// When set, `gr status` shows a `Bkp` column: is each repo's home present on
    /// the backup too? Presence only — replication lag / snapshot freshness are the
    /// backup host's own monitor (e.g. a fleet healthcheck), not observable here.
    pub backup: Server,
    /// Audit logging (ADR-0004, AU).
    pub audit: AuditConfig,
}

/// Audit-log settings. On by default; logs to `$XDG_STATE_HOME/git-redundancy/
/// audit.log` (fallback `~/.local/state/...`) unless `log` overrides the path.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AuditConfig {
    pub enabled: bool,
    pub log: Option<PathBuf>,
}

impl Default for AuditConfig {
    fn default() -> Self {
        AuditConfig {
            enabled: true,
            log: None,
        }
    }
}

/// How `push` chooses remotes. With `auto = true`, the remotes in `order` are
/// treated as interchangeable paths to the *same* server (e.g. `data-lan` over
/// LAN, `data` over Tailscale) — push tries them in order until one succeeds, so
/// you back up once, preferring the LAN. With `auto = false`, push targets each
/// remote independently.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Transport {
    pub auto: bool,
    pub order: Vec<String>,
}

impl Default for Transport {
    fn default() -> Self {
        Transport {
            auto: true,
            order: Vec::new(),
        }
    }
}

/// Where the bare "home" repos live on the server, and how to reach them
/// (ADR-0012). An empty `root` means server features are off — `gr` stays
/// purely local. Home-only repos have no local remote to read connection
/// details from, so the server coordinates live here, not only in git remotes.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Server {
    /// Bare-repo root on the server, e.g. `/data/git`. Empty = inventory disabled.
    pub root: PathBuf,
    /// SSH aliases to reach the server, tried in order (ADR-0009). Empty = derive
    /// from the `transport.order` remotes of discovered repos.
    pub aliases: Vec<String>,
}

impl Config {
    /// `$XDG_CONFIG_HOME/git-redundancy/config.toml`, falling back to `~/.config/...`.
    pub fn config_path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        base.join("git-redundancy").join("config.toml")
    }

    /// Load from the default location. A missing file is not an error — it yields
    /// an empty config (nothing to do), never a surprise.
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                toml::from_str(&s).with_context(|| format!("parsing config at {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading config at {}", path.display())),
        }
    }

    /// Nothing configured to act on.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty() && self.repos.is_empty()
    }

    /// Is the server-side home inventory configured (ADR-0012)? False = `gr`
    /// stays purely local.
    pub fn server_enabled(&self) -> bool {
        !self.server.root.as_os_str().is_empty()
    }

    /// Is a backup home server configured? It needs both a `root` and explicit
    /// `aliases` (there's no per-repo backup remote to derive them from).
    pub fn backup_enabled(&self) -> bool {
        !self.backup.root.as_os_str().is_empty() && !self.backup.aliases.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty_not_error() {
        let cfg = Config::load_from(Path::new("/nonexistent/git-redundancy/config.toml")).unwrap();
        assert_eq!(cfg, Config::default());
        assert!(cfg.is_empty());
    }

    #[test]
    fn parses_toml() {
        let toml = r#"
            roots = ["/data/Development"]
            default_remotes = ["data-lan", "data"]
        "#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.roots, vec![PathBuf::from("/data/Development")]);
        assert_eq!(cfg.default_remotes, vec!["data-lan", "data"]);
        assert!(!cfg.is_empty());
    }

    #[test]
    fn server_block_parses_and_toggles_inventory() {
        let cfg: Config = Config::default();
        assert!(!cfg.server_enabled());

        let toml = r#"
            roots = ["/data/Development"]
            [server]
            root = "/data/git"
            aliases = ["tenx-lan", "tenx-ts"]
        "#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.server_enabled());
        assert_eq!(cfg.server.root, PathBuf::from("/data/git"));
        assert_eq!(cfg.server.aliases, vec!["tenx-lan", "tenx-ts"]);
    }

    #[test]
    fn backup_block_parses_and_needs_root_plus_aliases() {
        assert!(!Config::default().backup_enabled());

        // root without aliases is not enough (nothing to derive aliases from).
        let no_aliases: Config = toml::from_str("[backup]\nroot = \"/data/git\"\n").unwrap();
        assert!(!no_aliases.backup_enabled());

        let toml = r#"
            roots = ["/data/Development"]
            [backup]
            root = "/data/git"
            aliases = ["acer-lan", "acer-ts"]
        "#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.backup_enabled());
        assert_eq!(cfg.backup.root, PathBuf::from("/data/git"));
        assert_eq!(cfg.backup.aliases, vec!["acer-lan", "acer-ts"]);
    }
}
