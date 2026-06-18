# 2026-06-17 — First implementation (status, push, audit)

**Documents:** commit `4a330c9` ("initial implementation"). Follows `e33c60c` (journal init)
and `59dcf06` (design). Second entry today — see the
[kickoff entry](2026-06-17-kickoff-and-design.md) for the design this builds on.
**Status:** first working increment — `gr status` and `gr push` work end-to-end with audit
logging. Not yet wired to the live tenx SSH aliases, so no real backup to tenx has run yet.

## What landed

The full Cargo workspace (20 files, ~2070 lines), three crates per ADR-0002,
`#![forbid(unsafe_code)]` throughout:

- **`git-redundancy-core`** (pure, no IO) — `WorkingTree`/`AheadBehind` types, `BranchSync`
  classification with `is_easy_push`, the `git status --porcelain=v2 -z` parser, and a
  dependency-free RFC3339 UTC formatter for the audit log. Unit tests + `proptest` on the
  invariants.
- **`git-redundancy-io`** (shell) — config-first loading (missing file = empty, never a
  scan), root/repo discovery, system-`git` local reads (branch / status / remotes /
  ahead-behind / `merge-tree`), `git push`, and the append-only audit writer.
- **`gr` (cli)** — clap surface; `status` table (per-remote `↑/↓`, `new`, `diverged`,
  `CONFLICT`; `--remote`, `-a/--all-branches`); `push` (easy-only, never force, never
  auto-commit, diverged/behind skipped, dirty surfaced; LAN→Tailscale failover;
  `--remote`/`--only`/`--dry-run`/`--tags`).

Gates green: build, `clippy --all-targets -- -D warnings`, `cargo test` (17 tests).

## How decisions showed up in the code

- **ADR-0002** functional-core/imperative-shell holds cleanly: all the tested logic lives in
  `core`; everything that touches the world is in `io`.
- **ADR-0006** — `push` is provably conservative: `is_easy_push` gates to ff/new only, no
  `--force` ever, no auto-commit; diverged (incl. `merge-tree` conflict detection) and
  behind are skipped and reported.
- **ADR-0009** — transport failover implemented: with `[transport].auto`, the ordered
  remotes are tried until one succeeds (verified: a push went out via `data-lan` and left
  `data` untouched).
- **ADR-0004 (AU)** — every real push action is audit-logged with a UTC timestamp;
  `--dry-run` performs no action and is deliberately not audited.

## Verification (by hand, this session)

- `gr status` against the real `/data/Development` repos matched reality (`api-server`
  `↑2`, dirty `local-notes`, etc.).
- `gr push` walked through a local bare remote: `new` → `--dry-run` (changed nothing) →
  real push (via `data-lan`) → fast-forward `↑1` → `up-to-date` + dirty warning → forced a
  divergence and confirmed `CONFLICT` is **SKIPPED, never forced**.
- Audit log: appends one line per action, RFC3339 UTC; dry-run wrote nothing.

## Known deviations / debt (be honest)

- **`gix` is not yet used for local reads.** ADR-0003 specified gix (pure Rust) for local
  reads with system `git` only for network ops. The current implementation shells out to
  system `git` for *both* reads and the push. The functional split and the
  single-network-chokepoint property still hold, but the "smallest pure-Rust trust base"
  benefit isn't realized yet — needs either a gix swap for reads or an ADR-0003 update.
- **No live SSH aliases yet** (ADR-0009) — so push hasn't run against the real tenx.
- **Integration tests not codified** — the push lifecycle above was exercised by hand, not
  yet captured in `assert_cmd` tests.

## Next

1. Wire the ADR-0009 SSH aliases + host-key pin → first real end-of-day backup to tenx.
2. Codify the integration tests (`assert_cmd` + `tempfile`).
3. `kani` proof job for the core classifiers; CI supply-chain gates (`deny`/`audit`/`vet`).
4. Reconcile the gix deviation (swap reads to gix, or update ADR-0003).
