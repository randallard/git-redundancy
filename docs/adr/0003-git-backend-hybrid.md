# ADR-0003: Git backend — hybrid (gix local read + system `git` for network)
- Status: Superseded by [ADR-0010](0010-system-git-for-local-reads.md)
- Date: 2026-06-17
- Deciders: Ryan

> **Superseded:** the gix-for-local-reads part was reversed — system `git` is used for
> reads too (see [ADR-0010](0010-system-git-for-local-reads.md)). The rest of this ADR's
> reasoning (reject `git2`/libgit2; single network chokepoint) still stands.

## Context
Three backends were weighed:

| Backend | Memory-safe / "provable" | FIPS crypto path | Push maturity |
|---|---|---|---|
| Shell out to system `git`+`ssh` | weakest (subprocess, parse porcelain) | easiest — OS OpenSSH | solid |
| `git2` (libgit2, C) | C in the trust base — undercuts "provable" | libssh2 + OpenSSL | mature |
| `gitoxide` (`gix`, pure Rust) | best — smallest trust base | rustls/aws-lc-rs (newer) | status strong; **push maturing** |

Two forces dominate: (1) we want the bulk of the code pure-Rust and memory-safe; (2) FIPS
(ADR-0005) demands a **single, controllable crypto chokepoint** for the network transport.

## Decision
**Hybrid, split by network boundary:**

- **`gix` for local reads only** — ref discovery, working-tree status, branch enumeration,
  ahead/behind via local revwalk. **`gix` never touches the network**, so no crypto path
  runs through it.
- **System `git` for everything that crosses the network or needs plumbing maturity** —
  `git fetch` (refresh remote-tracking), `git push` (the write), and
  `git merge-tree --write-tree` (conflict detection; git ≥ 2.38).

No `libgit2`/`git2` (keeps C out of the trust base).

## Consequences
- Bulk of logic is pure-Rust and memory-safe; **all transport crypto funnels through one
  chokepoint** — the OS OpenSSH invoked by `git` — which ADR-0005 can enforce/audit.
- Avoids gix's still-maturing push path while keeping its excellent read story.
- Hard dependency on a system `git` ≥ 2.38 (for `merge-tree --write-tree`); detect at startup.
- We parse `git push`/`status` porcelain (`--porcelain`, `-z`); covered by integration tests.
