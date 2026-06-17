# ADR-0007: Future GUI via Tauri, keep the Rust core
- Status: Proposed (future phase)
- Date: 2026-06-17
- Deciders: Ryan

## Context
The stated future goal is a GUI that is "provable, testable, FISMA High, TypeScript."
TypeScript is **not** memory-safe or "provable" in the Rust sense, and a TS rewrite of the
logic would discard the verified core (ADR-0001/0002) and the FIPS chokepoint (ADR-0005).

## Decision (proposed)
When the GUI is built, **do not reimplement logic in TypeScript.** Keep `git-redundancy-core`
(provable, Kani-verified, FIPS-capable transport) and wrap it:

- **Tauri** (Rust backend + web/TS frontend) — *recommended*: reuses the Rust core directly;
  TS is only the view layer. Best alignment with all four goals.
- *or* compile `git-redundancy-core` to **WASM** and call it from a TS app (core stays Rust).

TS layer assurance: `strict` + `noUncheckedIndexedAccess`, ESLint, Vitest/Playwright e2e,
`npm audit` / `osv-scanner`, SBOM. FISMA-High *alignment* (ADR-0004) and FIPS posture
(ADR-0005) carry over because crypto and core logic stay in Rust.

## Consequences
- The provable/FIPS value lives in the Rust core; the GUI wraps it, never replaces it.
- "TypeScript" is satisfied at the view layer without weakening the security/proof story.
- Revisit and promote to Accepted when the GUI phase actually starts.
