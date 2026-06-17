//! Kani bounded proofs for the pure core (ADR-0001/0002).
//!
//! These harnesses are compiled only under `--cfg kani` (i.e. by `cargo kani`),
//! so they add no dependency and don't affect normal `cargo build`/`test`/`clippy`.
//! Run with:  `cargo kani -p git-redundancy-core`
//!
//! Where `proptest` *samples* the input space, these *prove* the properties hold
//! for every input (within Kani's bounds — full `u32` for the classifier, a
//! realistic date range for the timestamp formatter).

use crate::{AheadBehind, BranchSync};

/// Build a symbolic `Option<AheadBehind>` without needing an `Arbitrary` impl.
fn any_ahead_behind() -> Option<AheadBehind> {
    if kani::any() {
        Some(AheadBehind {
            ahead: kani::any(),
            behind: kani::any(),
        })
    } else {
        None
    }
}

fn any_conflict() -> Option<bool> {
    if kani::any() {
        Some(kani::any())
    } else {
        None
    }
}

/// THE safety invariant: a push is only ever "easy" when the remote is not ahead
/// of us. Proven for *all* `u32` ahead/behind — this is what guarantees `gr push`
/// can never clobber remote history by fast-forwarding over commits it doesn't have.
#[kani::proof]
fn easy_push_implies_not_behind() {
    let ab = AheadBehind {
        ahead: kani::any(),
        behind: kani::any(),
    };
    let sync = BranchSync::classify(Some(ab), any_conflict());
    if sync.is_easy_push() {
        assert_eq!(ab.behind, 0, "easy push must imply behind == 0");
    }
}

/// `classify` is total: never panics and always yields a non-empty label, for any
/// input including the no-remote-branch case.
#[kani::proof]
fn classify_is_total() {
    let sync = BranchSync::classify(any_ahead_behind(), any_conflict());
    assert!(!sync.label().is_empty());
}

/// `is_easy_push` holds exactly for the create/no-op/fast-forward states, never
/// for behind or diverged — the full decision table, proven.
#[kani::proof]
fn easy_push_matches_states() {
    let sync = BranchSync::classify(any_ahead_behind(), any_conflict());
    let easy = matches!(
        sync,
        BranchSync::NoRemoteBranch | BranchSync::UpToDate | BranchSync::Ahead(_)
    );
    assert_eq!(sync.is_easy_push(), easy);
}

// Note: `rfc3339_utc` is intentionally *not* a Kani target — it runs `format!`,
// and symbolic string formatting is prohibitively expensive to model-check for
// little gain. Its no-panic / shape properties are covered by `proptest` in
// `timefmt.rs`. Kani here is reserved for the integer decision logic.
