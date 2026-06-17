# ADR-0002: Functional core / imperative shell
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
"Provable + testable" is only achievable if the logic worth proving is isolated from IO.
You cannot meaningfully formally-verify a program that is mostly subprocess calls,
network, and filesystem. The classifiers that decide *what the table shows* and *whether a
push is safe* are pure functions and are exactly where bugs would be dangerous.

## Decision
Three crates:

- **`git-redundancy-core`** — pure, no IO: state types, ahead/behind & "easy push" classification,
  porcelain parsing, merge-state mapping, table model. Target of `proptest` + `kani`.
  `#![forbid(unsafe_code)]`, no IO dependencies.
- **`git-redundancy-io`** — imperative shell: git invocation, network, filesystem, config load,
  audit logging. Covered by integration tests, not formal proof.
- **`git-redundancy-cli`** — binary: `clap` parsing, rendering, wiring core↔io.

## Consequences
- The verifiable surface is small and real; proofs/property tests target deterministic
  pure functions.
- Clear test boundary: unit/proptest/kani on `core`; integration (`assert_cmd`) on `io`/`cli`.
- Cost: marshaling between the IO shell's raw data and the core's typed model adds some
  boilerplate.
