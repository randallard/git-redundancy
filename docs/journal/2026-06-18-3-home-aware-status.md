# 2026-06-18 — Home-aware status & detail view (ADR-0014)

**Documents:** commit `72e0775` ("ADR-0014"). Follows `10880f9` (journal for the lifecycle
commands) and `65ef6ff` (the commands themselves). Third entry today — completes the
home-awareness increment begun in the
[inventory entry](2026-06-18-home-inventory-and-a-course-correct.md).
**Status:** the 0012→0014 arc is feature-complete. `gr status` now shows both sides of every
repo and drills into one; `create`/`clone`/`sync` close the gaps it surfaces.

## What landed

`gr status` went from a local-only table to the home-aware view ADR-0014 specified:

- **Lifecycle column** (`linked` / `local-only` / `home-only` / `?`), `home-only` repos shown
  as their own rows, and a **`+N⚠`** "others need attention" hint — the count of *other*
  local branches that aren't up-to-date, so a clean current branch can't hide un-backed-up
  work.
- **`--offline`** skips the server query (lifecycle renders `?`); when tenx is unreachable the
  view degrades the same way and still exits 0.
- **`gr status <repo>`** — a positional all-branches **detail view** with a `sync`-action
  column (`push (new)` / `ff ↓n` / `diverged` / `clone` …), resolving the target by **home or
  directory name**, and listing home-only branches via one `ls-remote`. It doubles as a
  dry-run preview of what `gr sync` would do.

Plumbing: `render.rs` gained the lifecycle/indicator columns and a `detail_table`;
`io::git::ls_remote_heads` lists a home's branches without cloning.

## One design call worth recording

The fleet **Repo column shows the on-disk directory name**, not the home name. Identity is
still the home name internally (and `gr status <name>` accepts either), but displaying
`omarchy-setup` for a directory called `USCourts_setup` was more confusing than helpful in the
common case — and it would have surprised the existing tests. So: directory name for local
repos, home name only for home-only rows. The mismatch still resolves correctly both ways
(verified live: `gr status omarchy-setup` opens the `USCourts_setup` checkout).

## Verification

- **Live against tenx:** the fleet showed `cmecf_* → linked`, `authentik`/`git-redundancy →
  local-only`, `myproject → home-only`; `gr status omarchy-setup` drilled in by home name
  (action `push (new)`); `gr status myproject` listed the home-only branch with action
  `clone`.
- **Hermetic:** new cli tests for the offline `?` lifecycle, the `+N⚠` indicator, and the
  detail action column — on top of the existing fixtures.
- **Gates:** `fmt` + `clippy -D warnings` clean; **55 tests** (core 21 · io 14 · cli 20);
  Kani still green (the integer proofs are untouched; this increment is string/collection
  logic, proptest-covered). Coverage ~76% line — `core` 98–100%, the dips are the live-SSH
  paths.

## Honest debt

- **`gr homes` is now redundant** with the lifecycle column. Kept as a quick diagnostic for
  the moment; a later cleanup can retire it or make it a thin alias.
- **Coverage sits at ~76%** because the SSH-execution paths (`io::server`, `create`/`clone`
  orchestration) only run against a live server. Same inherent limit as the rest of the
  network code; an SSH stub would be the way to close it.

## Next

Decision phase is done for this arc; the open work is the backlog in
[PROGRESS](../PROGRESS.md) §"Not yet" / §6 — `--json` output, a CI coverage gate (the
`cargo-llvm-cov` tool is now wired locally; see the new
[DEVELOPMENT.md](../DEVELOPMENT.md)), `cargo-vet` + SBOM, and the deferred *mandatory* FIPS
tier (server-side `sshd`/crypto-policy). Plus the operational item of keeping tenx awake/
reachable at day's end.
