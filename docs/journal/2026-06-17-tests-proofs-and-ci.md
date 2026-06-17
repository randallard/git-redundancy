# 2026-06-17 — Tests, proofs, and the CI decision

**Documents:** commit `c4930ad` (Kani proofs + ADR-0011); also covers, as a milestone since
the [first-implementation entry](2026-06-17-first-implementation.md) (`cf31b00`), commits
`3d25772` (gix reconciliation) and `3f945de` (integration tests). Third entry today.
**Status:** status/push behavior now pinned by hermetic tests; the core safety invariant is
*formally proven*; the gix deviation is reconciled; CI strategy decided but not yet wired.

## What happened since the first implementation

- **Reconciled the gix/ADR-0003 deviation (`3d25772`).** The first implementation shelled
  out to system `git` for local reads, not `gix` as ADR-0003 specified. On review the
  premise had flipped: `git` is already mandatory (push + `merge-tree`), so `gix` would
  *enlarge* the supply-chain graph rather than shrink the trust base, and system `git` gives
  exact config fidelity and one code path. Decided in the code's favor — **ADR-0010**
  supersedes 0003; `git2`/libgit2 stays rejected; the single-network-chokepoint holds.
- **Integration tests (`3f945de`).** 8 hermetic `assert_cmd` cases (isolated HOME +
  disabled global/system git config + isolated XDG) codify the lifecycle we'd been checking
  by hand: empty-config, new-branch, dry-run-changes-nothing-and-not-audited, push →
  up-to-date with failover + audit, fast-forward, **diverged-CONFLICT skip (exit 0)**,
  dirty-warn, and **real-failure exit 1**.
- **Kani proofs (`c4930ad`), verified green.** 3 cfg-gated harnesses over the integer
  decision logic — headline: **`is_easy_push ⇒ behind == 0` proven for all `u32`**, the
  formal guarantee that `gr push` can never fast-forward over commits it doesn't have. Plus
  `classify` totality and the easy-push decision table. `cargo kani -p git-redundancy-core`
  → 3/3, 0 failures, 0.04s.
- **CI decided — ADR-0011.** No deploy ⇒ CI is a verification/notification net. Public repo
  ⇒ free minutes. So: fast gates (fmt/clippy/test/deny/audit) + a *separate* Kani job, both
  **on every push**, Kani cached (`~/.kani` + nightly) to keep per-run cost to seconds.
  Workflows not written yet.

## Lessons worth keeping

- **Kani needs `rustup`, not the Arch `rust` package.** `cargo kani setup` died at
  `rustup toolchain install … No such file or directory` — first mis-blamed on Zscaler, but
  it was the missing `rustup` exec. Switched via `pacman -S rustup`. Documented in
  TROUBLESHOOTING so the red herring doesn't recur.
- **Kani's sweet spot is integer logic; `format!` is not.** The `rfc3339_utc` harness ran
  forever (symbolic `String`/`Vec` allocation — visible as floods of `raw_vec` /
  `capacity_overflow` path-aborts). Dropped it from Kani; `proptest` covers the formatter.
  Rule of thumb going forward: prove the pure decision logic, property-test the
  parsing/formatting.
- **The "provable" thesis delivered:** the safety invariant is now a proof, not a sample —
  the thing that makes this more than a normal CLI.

## Next

1. **SSH aliases + host-key pin (ADR-0009)** — the last piece before a real end-of-day backup
   to tenx over the FIPS-enforced path.
2. Write the CI workflows implementing ADR-0011.
