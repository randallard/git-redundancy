//! Joining a repo's two presences — local working copy and bare "home" on the
//! server — into a lifecycle (ADR-0012). Pure: the identity and join rules live
//! here; the SSH / `git ls-remote` IO that gathers the inputs is the imperative
//! shell (`git_redundancy_io::inventory`).

use std::collections::{BTreeMap, BTreeSet};

/// Which sides of a repo exist (ADR-0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Working copy here, no bare home yet — needs `create`.
    LocalOnly,
    /// Bare home on the server, never cloned here — needs `clone`.
    HomeOnly,
    /// Both present — compare per-branch drift with `sync`.
    Linked,
}

impl Lifecycle {
    /// Short label for status output.
    pub fn label(&self) -> &'static str {
        match self {
            Lifecycle::LocalOnly => "local-only",
            Lifecycle::HomeOnly => "home-only",
            Lifecycle::Linked => "linked",
        }
    }
}

/// A local working copy as the join sees it: its directory name and the home
/// name recovered from its `data` remote URL (`None` when it has no home remote).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRepo {
    pub dir_name: String,
    pub home_name: Option<String>,
}

/// One repo's identity and which sides exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPresence {
    /// Stable identity — the bare-home name (`<root>/<home_name>.git`).
    pub home_name: String,
    /// Local working-copy directory name, when a local copy exists.
    pub local_dir: Option<String>,
    pub lifecycle: Lifecycle,
}

/// Derive a home name from a remote URL: the final path segment without `.git`.
/// `ssh://tenx-lan/data/git/omarchy-setup.git` → `omarchy-setup`. Handles a
/// trailing slash and scp-like `host:path` forms; `None` if nothing is left.
pub fn home_name_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_string())
}

/// Join discovered local repos with the server's home names into a sorted,
/// de-duplicated lifecycle view (ADR-0012).
///
/// A local repo's *effective* home name is the one from its remote, or its
/// directory name when it has no home yet. A match against `homes` is `Linked`;
/// an unmatched local is `LocalOnly`; a home with no local is `HomeOnly`.
/// Output is keyed and sorted by home name, so each home appears exactly once
/// (the two transports `data`/`data-lan` resolve to one name upstream).
pub fn join_presences(locals: &[LocalRepo], homes: &[String]) -> Vec<RepoPresence> {
    let home_set: BTreeSet<&str> = homes.iter().map(String::as_str).collect();
    let mut out: BTreeMap<String, RepoPresence> = BTreeMap::new();

    for l in locals {
        let effective = l.home_name.clone().unwrap_or_else(|| l.dir_name.clone());
        let lifecycle = if home_set.contains(effective.as_str()) {
            Lifecycle::Linked
        } else {
            Lifecycle::LocalOnly
        };
        out.insert(
            effective.clone(),
            RepoPresence {
                home_name: effective,
                local_dir: Some(l.dir_name.clone()),
                lifecycle,
            },
        );
    }

    for h in homes {
        out.entry(h.clone()).or_insert_with(|| RepoPresence {
            home_name: h.clone(),
            local_dir: None,
            lifecycle: Lifecycle::HomeOnly,
        });
    }

    out.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(dir: &str, home: Option<&str>) -> LocalRepo {
        LocalRepo {
            dir_name: dir.to_string(),
            home_name: home.map(str::to_string),
        }
    }

    #[test]
    fn home_name_from_various_urls() {
        assert_eq!(
            home_name_from_url("ssh://tenx-lan/data/git/omarchy-setup.git").as_deref(),
            Some("omarchy-setup")
        );
        assert_eq!(
            home_name_from_url("ssh://tenx-ts/data/git/proj.git/").as_deref(),
            Some("proj")
        );
        assert_eq!(
            home_name_from_url("randallard@host:repos/foo.git").as_deref(),
            Some("foo")
        );
        // No `.git` suffix is fine — take the last segment as-is.
        assert_eq!(
            home_name_from_url("ssh://h/a/b/bare").as_deref(),
            Some("bare")
        );
        assert_eq!(home_name_from_url(""), None);
        assert_eq!(home_name_from_url("/"), None);
    }

    #[test]
    fn linked_when_local_home_name_matches_a_server_home() {
        // Directory name differs from the home name (USCourts_setup ↔ omarchy-setup).
        let locals = [local("USCourts_setup", Some("omarchy-setup"))];
        let homes = ["omarchy-setup".to_string()];
        let v = join_presences(&locals, &homes);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].home_name, "omarchy-setup");
        assert_eq!(v[0].local_dir.as_deref(), Some("USCourts_setup"));
        assert_eq!(v[0].lifecycle, Lifecycle::Linked);
    }

    #[test]
    fn local_without_home_is_local_only_keyed_by_dir_name() {
        let locals = [local("fresh", None)];
        let v = join_presences(&locals, &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].home_name, "fresh");
        assert_eq!(v[0].lifecycle, Lifecycle::LocalOnly);
    }

    #[test]
    fn home_without_local_is_home_only() {
        let homes = ["server-only".to_string()];
        let v = join_presences(&[], &homes);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].home_name, "server-only");
        assert_eq!(v[0].local_dir, None);
        assert_eq!(v[0].lifecycle, Lifecycle::HomeOnly);
    }

    #[test]
    fn mixed_fleet_is_sorted_and_deduped() {
        let locals = [
            local("USCourts_setup", Some("omarchy-setup")),
            local("brand-new", None),
        ];
        let homes = ["omarchy-setup".to_string(), "cmecf_inside".to_string()];
        let v = join_presences(&locals, &homes);
        let states: Vec<_> = v
            .iter()
            .map(|p| (p.home_name.as_str(), p.lifecycle))
            .collect();
        assert_eq!(
            states,
            vec![
                ("brand-new", Lifecycle::LocalOnly),
                ("cmecf_inside", Lifecycle::HomeOnly),
                ("omarchy-setup", Lifecycle::Linked),
            ]
        );
    }

    proptest::proptest! {
        /// Every home name appears exactly once, and every input home is present
        /// in the output (no home is ever dropped or duplicated).
        #[test]
        fn join_covers_all_homes_once(
            homes in proptest::collection::vec("[a-c]{1,3}", 0..6)
        ) {
            let v = join_presences(&[], &homes);
            let names: BTreeSet<&str> = v.iter().map(|p| p.home_name.as_str()).collect();
            let inputs: BTreeSet<&str> = homes.iter().map(String::as_str).collect();
            proptest::prop_assert_eq!(names, inputs);
            proptest::prop_assert_eq!(v.len(), v.iter().map(|p| &p.home_name).collect::<BTreeSet<_>>().len());
        }

        /// Full join invariants over arbitrary mixes of locals (with/without a
        /// home name) and server homes — the small alphabet forces collisions and
        /// matches so the branches actually exercise.
        #[test]
        fn join_lifecycle_invariants(
            raw_locals in proptest::collection::vec(
                ("[a-d]{1,2}", proptest::option::of("[a-d]{1,2}")),
                0..6),
            homes in proptest::collection::vec("[a-d]{1,2}", 0..6),
        ) {
            let locals: Vec<LocalRepo> = raw_locals
                .iter()
                .map(|(dir, home)| LocalRepo {
                    dir_name: dir.clone(),
                    home_name: home.clone(),
                })
                .collect();
            let v = join_presences(&locals, &homes);
            let home_set: BTreeSet<&str> = homes.iter().map(String::as_str).collect();

            // Output is strictly ascending by home name ⇒ sorted and unique.
            for w in v.windows(2) {
                proptest::prop_assert!(w[0].home_name < w[1].home_name);
            }

            // Coverage: every local's effective name and every home is represented.
            for l in &locals {
                let effective = l.home_name.clone().unwrap_or_else(|| l.dir_name.clone());
                proptest::prop_assert!(v.iter().any(|p| p.home_name == effective));
            }
            for h in &homes {
                proptest::prop_assert!(v.iter().any(|p| &p.home_name == h));
            }

            // Per-entry lifecycle is exactly determined by which sides exist.
            for p in &v {
                let on_server = home_set.contains(p.home_name.as_str());
                match p.lifecycle {
                    Lifecycle::Linked => {
                        proptest::prop_assert!(p.local_dir.is_some() && on_server);
                    }
                    Lifecycle::LocalOnly => {
                        proptest::prop_assert!(p.local_dir.is_some() && !on_server);
                    }
                    Lifecycle::HomeOnly => {
                        proptest::prop_assert!(p.local_dir.is_none() && on_server);
                    }
                }
            }
        }
    }
}
