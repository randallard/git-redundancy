//! Classifying what an un-redundant repo needs to become redundant (ADR-0020).
//!
//! Pure: given only *where the home already lives* — on the primary `[server]`,
//! on the `[backup]`, both, or neither — decide which onboarding mechanism
//! applies. The SSH listing that discovers those homes is the imperative shell
//! (`git_redundancy_io`); the decision itself is a total function of two bits and
//! lives here so it is provable without a network.

/// The mechanism `gr onboard` uses to make one repo redundant (ADR-0017/0020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardAction {
    /// No home anywhere → `create` provisions the full topology from scratch.
    Create,
    /// A home exists on the **backup** but not the primary — the original-7
    /// sub-state from before the fleet flipped → `repoint` (ADR-0018) provisions
    /// the primary and re-roles the existing backup.
    Repoint,
    /// A home already exists on the **primary** — the working copy simply was
    /// never wired to it (no `data` remote) → `attach` (ADR-0020) wires the local
    /// remotes at the existing primary and reconciles; no fresh home is created.
    Attach,
}

/// Decide the onboarding mechanism from where the home already lives.
///
/// The primary is decisive: **if a primary home exists, the answer is always
/// `Attach`**, regardless of the backup — a fresh `create` would refuse ("already
/// exists") and a `repoint` has nothing to provision (its whole job is a *missing*
/// primary). Only when the primary is absent does the backup break the tie:
/// present → `Repoint` (re-role it under a new primary), absent → `Create`.
pub fn classify_onboard(primary_home: bool, backup_home: bool) -> OnboardAction {
    match (primary_home, backup_home) {
        (true, _) => OnboardAction::Attach,
        (false, true) => OnboardAction::Repoint,
        (false, false) => OnboardAction::Create,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_home_anywhere_is_create() {
        assert_eq!(classify_onboard(false, false), OnboardAction::Create);
    }

    #[test]
    fn backup_only_is_repoint() {
        assert_eq!(classify_onboard(false, true), OnboardAction::Repoint);
    }

    #[test]
    fn primary_present_is_always_attach() {
        // The bug ADR-0020 fixes: a primary home already exists (backup present or
        // not), but the old classifier — blind to the primary — mis-routed this to
        // `repoint` (backup present) or `create` (a fresh home). Both dead-end.
        assert_eq!(classify_onboard(true, true), OnboardAction::Attach);
        assert_eq!(classify_onboard(true, false), OnboardAction::Attach);
    }
}
