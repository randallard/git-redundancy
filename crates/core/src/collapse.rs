//! Collapse same-server transport aliases into one logical status column
//! (ADR-0021). `gr status` renders one column per shown remote, but a transport
//! failover group (`[transport].order`, e.g. `["data-lan", "data"]`) is *not*
//! two destinations — it is two wires to the **same** server. This module is the
//! pure decision: given the shown remotes and the group, produce the collapsed
//! column layout, and fold a row's per-alias cells into one truthful cell. The
//! shell supplies the (already fetched, ADR-0019) per-alias cells; the fold is a
//! total function proved here without a network (ADR-0002).

use crate::BranchSync;

/// A rank where **higher = more of our work is safely on the server**. Used to
/// pick the truthful cell when two transports to the *same* server disagree —
/// the realistic case being a stale sibling ref after a failover push, where one
/// alias reads `ok` and the other a phantom `↑n`. `ok` must win; this order makes
/// it so, and resolves every other tie toward the safer state. Total order.
fn backup_safety(s: &BranchSync) -> (u8, i64) {
    match s {
        // Server has exactly our work — fully backed up.
        BranchSync::UpToDate => (5, 0),
        // Server has all our commits and then some — our work is safe; fewer
        // behind sorts higher only to make the order total.
        BranchSync::Behind(n) => (4, -(*n as i64)),
        // Server is missing `n` of our commits — a real gap; fewer missing is
        // more backed up, so a stale `↑n` loses to a fresh `ok` (rank 5 > 3).
        BranchSync::Ahead(n) => (3, -(*n as i64)),
        // The branch isn't on the server at all — nothing of it is backed up.
        BranchSync::NoRemoteBranch => (2, 0),
        // Diverged: our work is partly un-backed and history conflicts; a plain
        // divergence outranks a merge-conflicting one.
        BranchSync::Diverged {
            conflict: false,
            ahead,
            ..
        } => (1, -(*ahead as i64)),
        BranchSync::Diverged { conflict: true, .. } => (0, 0),
    }
}

/// Of two cells for the *same* server (different transports), the one showing the
/// server holding the most of our work. On a tie, `a` wins (the earlier alias).
pub fn most_backed_up(a: BranchSync, b: BranchSync) -> BranchSync {
    if backup_safety(&a) >= backup_safety(&b) {
        a
    } else {
        b
    }
}

/// A plan for rendering `shown` remotes as collapsed columns: the display labels
/// and, per output column, which indices of the original `shown` feed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnCollapse {
    /// Display column labels, aligned with the output of [`ColumnCollapse::fold`].
    pub remotes: Vec<String>,
    /// For each output column, the indices into the original `shown` it folds.
    sources: Vec<Vec<usize>>,
}

impl ColumnCollapse {
    /// Fold one row's per-`shown` cells into the collapsed columns. Each output
    /// cell is the [`most_backed_up`] of its sources' *present* cells, and `None`
    /// only when every source alias is absent (so a cloud-only repo shows one `-`
    /// instead of two).
    pub fn fold(&self, cells: &[Option<BranchSync>]) -> Vec<Option<BranchSync>> {
        self.sources
            .iter()
            .map(|idxs| {
                idxs.iter()
                    .filter_map(|&i| cells.get(i).copied().flatten())
                    .reduce(most_backed_up)
            })
            .collect()
    }
}

/// Collapse the transport-alias `group` (interchangeable wires to one server,
/// e.g. `["data-lan", "data"]`) within `shown` into a single logical column,
/// while every non-group remote (`backup`, `origin`, …) keeps its own column in
/// place. The collapsed column sits where the group *first* appears and is
/// labeled by the group's canonical member — the last one in `group` order that
/// is actually shown (conventionally the portable `data`). (ADR-0021.)
pub fn collapse_columns(shown: &[String], group: &[String]) -> ColumnCollapse {
    let in_group = |r: &str| group.iter().any(|g| g == r);
    // Canonical label: the group member appearing last in `group` order that is
    // actually present in `shown`.
    let label = group
        .iter()
        .rev()
        .find(|g| shown.iter().any(|s| s == *g))
        .cloned();

    let mut remotes = Vec::new();
    let mut sources: Vec<Vec<usize>> = Vec::new();
    let mut group_col: Option<usize> = None;
    for (i, r) in shown.iter().enumerate() {
        if in_group(r) {
            let col = *group_col.get_or_insert_with(|| {
                remotes.push(label.clone().unwrap_or_else(|| r.clone()));
                sources.push(Vec::new());
                remotes.len() - 1
            });
            sources[col].push(i);
        } else {
            remotes.push(r.clone());
            sources.push(vec![i]);
        }
    }
    ColumnCollapse { remotes, sources }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn two_aliases_become_one_data_column() {
        let plan = collapse_columns(&strs(&["data-lan", "data"]), &strs(&["data-lan", "data"]));
        assert_eq!(plan.remotes, strs(&["data"]));
    }

    #[test]
    fn non_group_remotes_keep_their_own_column_in_place() {
        let plan = collapse_columns(
            &strs(&["data-lan", "data", "origin"]),
            &strs(&["data-lan", "data"]),
        );
        assert_eq!(plan.remotes, strs(&["data", "origin"]));
    }

    #[test]
    fn group_column_sits_where_the_group_first_appears() {
        let plan = collapse_columns(
            &strs(&["origin", "data-lan", "data"]),
            &strs(&["data-lan", "data"]),
        );
        assert_eq!(plan.remotes, strs(&["origin", "data"]));
    }

    #[test]
    fn fold_prefers_ok_over_a_stale_ahead_sibling() {
        // The headline case: a failover push moved only `data-lan`'s ref, so
        // `data` still reads `↑1`. The collapsed cell must be the truthful `ok`.
        let plan = collapse_columns(&strs(&["data-lan", "data"]), &strs(&["data-lan", "data"]));
        let folded = plan.fold(&[Some(BranchSync::UpToDate), Some(BranchSync::Ahead(1))]);
        assert_eq!(folded, vec![Some(BranchSync::UpToDate)]);
        // Order-independent.
        let folded = plan.fold(&[Some(BranchSync::Ahead(1)), Some(BranchSync::UpToDate)]);
        assert_eq!(folded, vec![Some(BranchSync::UpToDate)]);
    }

    #[test]
    fn fold_is_none_only_when_every_alias_is_absent() {
        let plan = collapse_columns(
            &strs(&["data-lan", "data", "origin"]),
            &strs(&["data-lan", "data"]),
        );
        // Cloud-only repo: neither transport present → one `-`, plus origin.
        let folded = plan.fold(&[None, None, Some(BranchSync::UpToDate)]);
        assert_eq!(folded, vec![None, Some(BranchSync::UpToDate)]);
        // Only the LAN alias wired → its cell survives the fold.
        let folded = plan.fold(&[Some(BranchSync::Ahead(2)), None, None]);
        assert_eq!(folded, vec![Some(BranchSync::Ahead(2)), None]);
    }

    #[test]
    fn a_genuine_gap_still_surfaces_when_all_aliases_agree() {
        // Not a stale artifact: the branch really isn't pushed, so both aliases
        // read `↑3`. The collapse must keep showing it, not hide it.
        let plan = collapse_columns(&strs(&["data-lan", "data"]), &strs(&["data-lan", "data"]));
        let folded = plan.fold(&[Some(BranchSync::Ahead(3)), Some(BranchSync::Ahead(3))]);
        assert_eq!(folded, vec![Some(BranchSync::Ahead(3))]);
    }

    #[test]
    fn ok_beats_diverged_conflict_between_same_server_transports() {
        assert_eq!(
            most_backed_up(
                BranchSync::UpToDate,
                BranchSync::Diverged {
                    ahead: 1,
                    behind: 1,
                    conflict: true,
                },
            ),
            BranchSync::UpToDate
        );
    }
}
