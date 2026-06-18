//! `git-redundancy-core` — the pure functional core (ADR-0002).
//!
//! No IO, no subprocesses, no network: just value types and the deterministic
//! logic that decides what the status table shows and whether a push is safe.
//! This is the crate that property tests and (later) `kani` target.
#![forbid(unsafe_code)]

pub mod classify;
pub mod model;
pub mod presence;
pub mod status;
pub mod timefmt;

/// Kani proof harnesses — compiled only under `--cfg kani` (`cargo kani`).
#[cfg(kani)]
mod proofs;

pub use classify::BranchSync;
pub use model::{AheadBehind, WorkingTree};
pub use presence::{home_name_from_url, join_presences, Lifecycle, LocalRepo, RepoPresence};
pub use status::parse_porcelain_v2_z;
pub use timefmt::rfc3339_utc;
