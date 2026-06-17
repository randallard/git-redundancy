//! Discover repos *within* configured roots, plus explicit repos, minus excludes.
//! Roots are always explicit — there is no implicit/global filesystem scan.

use crate::config::Config;
use std::path::{Path, PathBuf};

/// A path is a repo if it contains a `.git` (directory for normal repos, file
/// for worktrees/submodules — `exists()` covers both).
fn is_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Resolve the configured roots/repos into a sorted, de-duplicated repo list.
pub fn discover(cfg: &Config) -> Vec<PathBuf> {
    let is_excluded = |p: &Path| {
        cfg.exclude
            .iter()
            .any(|e| p == e.as_path() || p.starts_with(e))
    };

    let mut out: Vec<PathBuf> = Vec::new();

    // Explicitly listed repos.
    for r in &cfg.repos {
        if is_repo(r) && !is_excluded(r) {
            out.push(r.clone());
        }
    }

    // Immediate children of each root that are repos.
    for root in &cfg.roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue; // unreadable/missing root is skipped, not fatal
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if is_repo(&p) && !is_excluded(&p) {
                out.push(p);
            }
        }
    }

    out.sort();
    out.dedup();
    out
}
