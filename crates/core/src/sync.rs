//! What `gr sync` would do for one branch (ADR-0013). Pure: given a branch's
//! sync classification and whether the working tree is clean, decide the single
//! safe action. The IO that carries it out (fetch / ff-merge / push) is the shell.
//!
//! Safety (ADR-0006 spirit, extended by ADR-0013): only fast-forwards and easy
//! pushes ever act; a fast-forward pull is gated on a clean working tree; diverged
//! / conflicted branches are reported, never forced, merged, or committed.

use crate::classify::BranchSync;

/// The one action `sync` takes for a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    /// Already in agreement — nothing to do.
    UpToDate,
    /// Local is ahead / the branch is new on the home → easy push.
    Push,
    /// Home is ahead and the tree is clean → fast-forward pull (`behind` commits).
    FastForward(u32),
    /// Home is ahead but the working tree is dirty → can't ff safely; report it.
    BlockedDirty(u32),
    /// Diverged / conflicted → manual; never forced or auto-merged.
    Report,
}

impl SyncAction {
    /// Decide the action from a branch's classification and the tree's cleanliness.
    /// `tree_clean` gates the fast-forward pull (the only working-tree write `sync`
    /// performs), so a dirty repo never gets its tree fast-forwarded out from under
    /// edits in progress.
    pub fn plan(sync: BranchSync, tree_clean: bool) -> SyncAction {
        match sync {
            BranchSync::UpToDate => SyncAction::UpToDate,
            BranchSync::NoRemoteBranch | BranchSync::Ahead(_) => SyncAction::Push,
            BranchSync::Behind(n) => {
                if tree_clean {
                    SyncAction::FastForward(n)
                } else {
                    SyncAction::BlockedDirty(n)
                }
            }
            BranchSync::Diverged { .. } => SyncAction::Report,
        }
    }

    /// Does this action change anything (and so want a confirmation under `-i`)?
    pub fn is_effecting(&self) -> bool {
        matches!(self, SyncAction::Push | SyncAction::FastForward(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_table() {
        assert_eq!(
            SyncAction::plan(BranchSync::UpToDate, true),
            SyncAction::UpToDate
        );
        assert_eq!(
            SyncAction::plan(BranchSync::Ahead(3), true),
            SyncAction::Push
        );
        assert_eq!(
            SyncAction::plan(BranchSync::NoRemoteBranch, false),
            SyncAction::Push
        );
        // Behind: fast-forward only on a clean tree, else blocked.
        assert_eq!(
            SyncAction::plan(BranchSync::Behind(2), true),
            SyncAction::FastForward(2)
        );
        assert_eq!(
            SyncAction::plan(BranchSync::Behind(2), false),
            SyncAction::BlockedDirty(2)
        );
        // Diverged is always reported, never acted on, clean tree or not.
        let diverged = BranchSync::Diverged {
            ahead: 1,
            behind: 1,
            conflict: false,
        };
        assert_eq!(SyncAction::plan(diverged, true), SyncAction::Report);
        assert_eq!(SyncAction::plan(diverged, false), SyncAction::Report);
    }

    #[test]
    fn only_push_and_ff_are_effecting() {
        assert!(SyncAction::plan(BranchSync::Ahead(1), true).is_effecting());
        assert!(SyncAction::plan(BranchSync::Behind(1), true).is_effecting());
        assert!(!SyncAction::plan(BranchSync::Behind(1), false).is_effecting());
        assert!(!SyncAction::plan(BranchSync::UpToDate, true).is_effecting());
    }

    proptest::proptest! {
        /// A fast-forward is only ever planned for a clean tree (the working-tree
        /// safety invariant), and a dirty tree never fast-forwards.
        #[test]
        fn ff_implies_clean_tree(ahead in 0u32..1000, behind in 0u32..1000, clean: bool) {
            let sync = BranchSync::classify(
                Some(crate::model::AheadBehind { ahead, behind }),
                Some(false),
            );
            if let SyncAction::FastForward(_) = SyncAction::plan(sync, clean) {
                proptest::prop_assert!(clean);
            }
            if !clean {
                proptest::prop_assert!(!matches!(SyncAction::plan(sync, clean), SyncAction::FastForward(_)));
            }
        }
    }
}
