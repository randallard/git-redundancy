//! Classify a local branch's relationship to one remote-tracking branch, and
//! decide whether pushing it is "easy" (ADR-0006).

use crate::model::AheadBehind;

/// Relationship of a local branch to one remote's copy of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchSync {
    /// The remote has no such branch yet; a push would create it.
    NoRemoteBranch,
    /// Identical to the remote.
    UpToDate,
    /// Local is ahead only — a push fast-forwards the remote.
    Ahead(u32),
    /// Local is behind only — a push would be rejected; you'd pull.
    Behind(u32),
    /// Histories diverged. `conflict` is the result of a trial merge (if known).
    Diverged { ahead: u32, behind: u32, conflict: bool },
}

impl BranchSync {
    /// Build from an optional ahead/behind (`None` = the remote branch is absent)
    /// and, when diverged, whether a trial merge reported conflicts.
    pub fn classify(ab: Option<AheadBehind>, diverged_conflict: Option<bool>) -> BranchSync {
        match ab {
            None => BranchSync::NoRemoteBranch,
            Some(AheadBehind { ahead: 0, behind: 0 }) => BranchSync::UpToDate,
            Some(AheadBehind { ahead, behind: 0 }) => BranchSync::Ahead(ahead),
            Some(AheadBehind { ahead: 0, behind }) => BranchSync::Behind(behind),
            Some(AheadBehind { ahead, behind }) => BranchSync::Diverged {
                ahead,
                behind,
                conflict: diverged_conflict.unwrap_or(false),
            },
        }
    }

    /// A push is "easy" iff it creates the branch or fast-forwards it — never
    /// when the remote is ahead (would be rejected) or histories diverged.
    /// `UpToDate` is trivially easy (a no-op).
    pub fn is_easy_push(&self) -> bool {
        matches!(
            self,
            BranchSync::NoRemoteBranch | BranchSync::UpToDate | BranchSync::Ahead(_)
        )
    }

    /// Short label for the status table.
    pub fn label(&self) -> &'static str {
        match self {
            BranchSync::NoRemoteBranch => "new",
            BranchSync::UpToDate => "ok",
            BranchSync::Ahead(_) => "ff",
            BranchSync::Behind(_) => "behind",
            BranchSync::Diverged { conflict: false, .. } => "diverged",
            BranchSync::Diverged { conflict: true, .. } => "CONFLICT",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_table() {
        assert_eq!(BranchSync::classify(None, None), BranchSync::NoRemoteBranch);
        assert_eq!(
            BranchSync::classify(Some(AheadBehind { ahead: 0, behind: 0 }), None),
            BranchSync::UpToDate
        );
        assert_eq!(
            BranchSync::classify(Some(AheadBehind { ahead: 3, behind: 0 }), None),
            BranchSync::Ahead(3)
        );
        assert_eq!(
            BranchSync::classify(Some(AheadBehind { ahead: 0, behind: 2 }), None),
            BranchSync::Behind(2)
        );
        assert_eq!(
            BranchSync::classify(Some(AheadBehind { ahead: 1, behind: 1 }), Some(true)),
            BranchSync::Diverged { ahead: 1, behind: 1, conflict: true }
        );
    }

    #[test]
    fn easy_push_only_when_ff_or_new() {
        assert!(BranchSync::NoRemoteBranch.is_easy_push());
        assert!(BranchSync::UpToDate.is_easy_push());
        assert!(BranchSync::Ahead(5).is_easy_push());
        assert!(!BranchSync::Behind(1).is_easy_push());
        assert!(!BranchSync::Diverged { ahead: 1, behind: 1, conflict: false }.is_easy_push());
    }

    proptest::proptest! {
        /// Core invariant (ADR-0006): a push is only ever "easy" when the remote
        /// is not ahead of us — i.e. behind == 0.
        #[test]
        fn easy_push_implies_not_behind(ahead in 0u32..10_000, behind in 0u32..10_000) {
            let s = BranchSync::classify(Some(AheadBehind { ahead, behind }), Some(false));
            if s.is_easy_push() {
                proptest::prop_assert_eq!(behind, 0);
            }
        }

        /// classify never panics and label is always non-empty.
        #[test]
        fn classify_total(ahead in 0u32..u32::MAX, behind in 0u32..u32::MAX, c in proptest::option::of(proptest::bool::ANY)) {
            let s = BranchSync::classify(Some(AheadBehind { ahead, behind }), c);
            proptest::prop_assert!(!s.label().is_empty());
        }
    }
}
