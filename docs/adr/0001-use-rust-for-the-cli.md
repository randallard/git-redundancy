# ADR-0001: Use Rust for the CLI
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
git-redundancy must be **memory-safe, testable, automatable, and "provable"** to a meaningful
degree, and ship as a fast single binary. Candidates considered: Rust, Go, Python, TypeScript.
"Provable" matters in tiers (memory safety → illegal-states-unrepresentable → property
testing → bounded formal proof); only Rust delivers the first tier *by construction*.

## Decision
Write the CLI in **Rust**, with `#![forbid(unsafe_code)]` across all crates.

Tooling baseline: `clap` (args), `serde`/`toml` (config), `tabled` + `owo-colors`
(table), `proptest` (property tests), `kani` (bounded proofs on the pure core),
`assert_cmd` + `tempfile` (integration), `cargo-llvm-cov` (coverage), and the supply-chain
gates in ADR-0004.

## Consequences
- **Provable memory safety for free**; no data races / use-after-free. Go/Python/TS can't match this.
- Best-in-class property-testing and bounded-verification ecosystem for the pure logic.
- Trivial distribution (static-ish binary), good cross-compile story.
- Cost: steeper development than Go/Python; more upfront type design.
- The investment is preserved into the GUI phase — see ADR-0007 (the Rust core is reused,
  not rewritten in TypeScript).
