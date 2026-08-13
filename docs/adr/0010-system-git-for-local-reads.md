# ADR-0010: System `git` for local reads too (supersedes ADR-0003)
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan
- Verified-by: `ci: fast gates` → `cargo-deny` `[bans] deny` in `deny.toml` — `git2`,
  `libgit2-sys` and `gix` are explicitly banned, so re-introducing libgit2 (directly *or*
  transitively, hence the `-sys` entry) fails the build. The ban mechanism was proven to fire,
  not just parse: a throwaway config banning `serde` (which *is* in the tree) produced
  `error[banned]` and exit 2. *(Was unenforced and true-by-luck from 2026-06-17 until
  2026-08-13 — the gap the `Verified-by` backfill surfaced, now closed.)*
  **Residual:** `gix-*` sub-crates are not individually banned; the umbrella `gix` is the
  realistic arrival path.
- Supersedes: [ADR-0003](0003-git-backend-hybrid.md)

## Context
ADR-0003 chose a hybrid: `gix` (pure Rust) for local reads, system `git` only for network
ops + `merge-tree`. The first implementation (`4a330c9`) instead used system `git` for
*both* reads and network. Reviewing the deviation, the original premise no longer holds:

- **System `git` ≥ 2.38 is already mandatory** — ADR-0003 itself requires it for `push` and
  `merge-tree --write-tree`. So `git` is a hard dependency regardless; using it for reads
  adds nothing new to the trust base.
- **`gix` would *enlarge* the supply-chain graph**, not shrink it: a large pure-Rust crate
  tree to vet/audit/SBOM (ADR-0004), versus zero added crates for shelling out to a `git`
  we already require and that is widely audited.
- **Config fidelity matters for a backup tool:** system `git` honors the user's exact
  config, includes, credential helpers, gitattributes, and alternates; `gix` may not
  replicate every nuance.
- **One mechanism = one failure mode:** reads and push both going through `git` is simpler
  than maintaining a gix read path plus a git push path.

`gix`'s real advantages — in-process speed and no subprocess — don't pay off at this scale
(~dozens of repos), and its pre-1.0 API adds upkeep.

## Decision
Use **system `git` for both local reads and network operations.** No `gix` dependency.

- The functional-core / imperative-shell split (ADR-0002) is unchanged: all read output is
  parsed by the pure `core`; `io` owns the subprocess calls.
- The single-network-chokepoint property (ADR-0003/0009) is unchanged: only `push`/`fetch`
  touch the network, so FIPS enforcement still lives in one place.
- `git2`/`libgit2` remains rejected (no C in the trust base).
- Reconsider `gix` only if read performance at large repo counts ever becomes a real need.

## Consequences
- **Zero added crates**; the dependency graph stays small for `cargo-deny`/`vet`/SBOM.
- **Exact git-config fidelity** and a single, consistent code path.
- **Hard dependency on system `git` ≥ 2.38** — already true; detect at startup.
- We forgo the pure-Rust read path and its in-process speed (acceptable here).
- The provable surface is unaffected — classification logic stays pure regardless of how
  bytes arrive.
