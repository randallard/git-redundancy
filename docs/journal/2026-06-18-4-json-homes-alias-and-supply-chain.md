# 2026-06-18 — `--json`, homes→status alias, and supply-chain gates

**Documents:** commit `4bfeb1a` (backlog items 2/3/5), with the dev-docs groundwork in
`285dfd7` ("update docs" — `DEVELOPMENT.md` + README). Follows `60499aa` (journal for
ADR-0014). Fourth entry today; the first one past the 0012→0014 feature arc — this is
backlog cleanup from [PROGRESS](../PROGRESS.md) §"Not yet" / §6, not new ADRs (both were
already decided: `--json` in ADR-0006, supply-chain in ADR-0004).
**Status:** the increment's loose ends are tied off — machine-readable status, a retired
`homes`, and the assurance gates the FISMA-aligned posture promised.

## What landed

Three items off the backlog:

- **`gr status --json`** (ADR-0006) — machine-readable output for both the fleet and the
  `<repo>` detail view, built from the *same* `render::Row`s the table renders (new
  `statusjson.rs`), so the two outputs can't drift. Shape:
  `{ remotes, repos: [{ repo, lifecycle, others?, branches: [{ branch, current, working_tree,
  remotes: {name → {state, ahead, behind, conflict}}, action? }] }] }`. This is the seam a
  future Tauri GUI consumes instead of scraping the table.
- **`gr homes` retired into a thin alias** for the fleet `status` view (ADR-0014) — its
  lifecycle list is now just a column there. `run_homes` deleted; its tests repurposed.
- **Supply-chain + coverage CI gates** (ADR-0004) — `cargo-vet` with a committed
  `supply-chain/` baseline (`cargo vet` → succeeds, 100 exempted; CI runs `--locked`), a
  CycloneDX **SBOM** artifact, and a **coverage gate** (`cargo llvm-cov --fail-under-lines
  70`). Two new CI jobs; commands documented in the new
  [DEVELOPMENT.md](../DEVELOPMENT.md).

## Notes / decisions

- **Coverage floor is 70%, not the ~76% headline.** The gap is the SSH-execution paths
  (`io::server`, `create`/`clone`) that only run against a live server and are verified by
  hand — the floor leaves room for them without pretending they're a hole.
- **`cargo-vet` baseline is exemptions, not audits.** `cargo vet init` exempted the current
  tree so the gate is green today; the ongoing work is auditing/re-exempting as deps change.
  That's the one item left in §6.
- **JSON is built from the render rows, deliberately.** Re-gathering would risk table/JSON
  divergence; grouping the flat rows back into per-repo objects keeps one source of truth.

## Honest debt

- The commit itself bundles all three items (and PROGRESS) into one — fine, but coarser than
  the per-item split we'd discussed.
- SBOM upload uses `if-no-files-found: warn`, so a filename-pattern mismatch in
  `cargo-cyclonedx` would silently skip rather than fail — worth a glance on the first CI run.

## Next

What's left in PROGRESS is no longer code-in-this-repo for the most part:

- **Operational:** keep `tenx` awake/reachable at day's end (the ADR-0009 orthogonal risk),
  and record the DHCP reservation — both are ops on the server, not the CLI.
- **Deferred tier:** *mandatory* FIPS (server-side `sshd`/crypto-policy), really a
  certified-platform move.
- **Future:** the Tauri GUI, now unblocked by `--json`.
