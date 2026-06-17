# ADR-0011: CI — fast gates always + Kani on every push (cached, separate job)
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
git-redundancy is run locally; there is **no cloud deploy**. So CI's job here is purely
**verification + notification**: catch the "changed the code and forgot to run
`cargo test` / `cargo kani` locally" case, fail the push, and notify the author (and any
watchers). The repo `randallard/git-redundancy` is **public**, so GitHub Actions minutes
are free — frequency of runs isn't a cost concern.

We deliberated whether to gate the (heavier) Kani job by `paths:`/schedule. Two findings:
- A `paths: crates/core/**` filter can be set too narrowly and **silently skip** a relevant
  change, defeating the safety-net purpose. Running every push removes that risk.
- The real per-run cost of Kani is **downloading** its CBMC bundle + pinned nightly, *not*
  solving (our handful of `core` harnesses solve in seconds). That cost is addressed by
  caching, independent of how often the job runs.

## Decision
Two GitHub Actions tiers, **both required for green**, both on every push/PR:

1. **Fast gates** — `cargo fmt --check`, `clippy --all-targets -D warnings`, `cargo test`,
   `cargo-deny` (licenses/bans/sources/advisories), `cargo-audit`. Seconds; the primary
   feedback loop. (Operationalizes ADR-0004 CM/SI/SR.)
2. **Proofs** — a *separate* job running `cargo kani -p git-redundancy-core` via the official
   `model-checking/kani-github-action` (version-pinned), with `actions/cache` on `~/.kani`
   and the pinned nightly toolchain, keyed on the Kani version. **Runs on every push** — no
   `paths` filter, no schedule.

Keeping Kani in its own job (rather than gating frequency) means: a slow/cranky proof run
never delays the fast-gate feedback, and the check name shows at a glance which failed.

## Consequences
- Forgetting to run tests or proofs locally is caught at push and surfaced to watchers.
- Free on the public repo; per-run Kani cost is ~seconds-plus-solve once the cache is warm.
- Small upkeep: pin the Kani action version and bump the cache key when Kani is upgraded.
- Kani requires `rustup` (not the Arch `rust` package) — local concern documented in
  [TROUBLESHOOTING](../TROUBLESHOOTING.md); CI uses the action's managed toolchain.
- **Revisit only if the proof suite grows slow** — then move Kani to PR-only or a nightly
  `schedule:`. Not warranted at the current handful of harnesses.
- Ties to ADR-0004 (gates), ADR-0001/0002 (the proofs these run), ADR-0010 (minimal deps
  that `cargo-deny` now enforces).
