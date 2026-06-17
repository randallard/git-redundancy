//! `git-redundancy-io` — the imperative shell (ADR-0002 / ADR-0003).
//!
//! Everything that touches the outside world: config loading, repo discovery,
//! and invoking system `git` for local reads. Network operations (fetch/push)
//! will live here too and funnel through one chokepoint (ADR-0009).
#![forbid(unsafe_code)]

pub mod audit;
pub mod config;
pub mod discovery;
pub mod git;

pub use audit::Audit;
pub use config::Config;
pub use discovery::discover;
