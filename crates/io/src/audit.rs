//! Append-only audit log of push actions (ADR-0004, AU). One line per action,
//! timestamped in UTC, recording what / where / result. No telemetry — local file only.

use crate::config::Config;
use anyhow::{Context, Result};
use git_redundancy_core::rfc3339_utc;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A resolved audit sink. `None` path = auditing disabled.
pub struct Audit {
    path: Option<PathBuf>,
}

impl Audit {
    pub fn from_config(cfg: &Config) -> Self {
        let path = if cfg.audit.enabled {
            Some(resolve_path(cfg))
        } else {
            None
        };
        Audit { path }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Append one record. No-op when auditing is disabled. Errors are returned so
    /// the caller can surface a failure to write the security log (fail loud).
    pub fn record(
        &self,
        repo: &str,
        branch: &str,
        remote: &str,
        result: &str,
        detail: &str,
    ) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating audit dir {}", parent.display()))?;
        }
        let stamp = rfc3339_utc(now_secs());
        let detail_field = if detail.is_empty() {
            String::new()
        } else {
            format!(" detail={detail:?}")
        };
        let line =
            format!("{stamp} action=push repo={repo} branch={branch} remote={remote} result={result}{detail_field}\n");

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening audit log {}", path.display()))?;
        file.write_all(line.as_bytes())
            .with_context(|| format!("writing audit log {}", path.display()))?;
        Ok(())
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn resolve_path(cfg: &Config) -> PathBuf {
    match &cfg.audit.log {
        Some(p) => expand_tilde(p),
        None => default_state_dir().join("git-redundancy").join("audit.log"),
    }
}

fn default_state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
}

fn expand_tilde(p: &Path) -> PathBuf {
    if let Ok(rest) = p.strip_prefix("~") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_audit_writes_nothing() {
        let mut cfg = Config::default();
        cfg.audit.enabled = false;
        let audit = Audit::from_config(&cfg);
        assert!(audit.path().is_none());
        // record is a no-op and must not error.
        audit.record("r", "b", "data-lan", "pushed", "↑1").unwrap();
    }

    #[test]
    fn appends_a_line() {
        let dir = std::env::temp_dir().join(format!("gr-audit-{}", std::process::id()));
        let log = dir.join("audit.log");
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = Config::default();
        cfg.audit.log = Some(log.clone());
        let audit = Audit::from_config(&cfg);

        audit
            .record("myrepo", "main", "data-lan", "pushed", "↑2")
            .unwrap();
        audit
            .record("myrepo", "main", "data-lan", "skipped", "diverged")
            .unwrap();

        let contents = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].contains("action=push repo=myrepo branch=main remote=data-lan result=pushed")
        );
        assert!(lines[0].ends_with("detail=\"↑2\""));
        assert!(lines[0].contains('T') && lines[0].contains('Z'));
        assert!(lines[1].contains("result=skipped"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
