# 2026-06-30 — `onboard` was dead-ending on the whole fleet: add `attach`

**Documents:** the code change adding the `attach` onboarding action (ADR-0020). Follows the
[repoint](2026-06-22-2-repoint-backup-only-homes.md) and
[stale-refs](2026-06-22-4-stale-tracking-refs-after-repoint.md) entries.
**Status:** ✅ implemented + tested (fmt/clippy clean, 71 tests green); **verified via `onboard
--dry-run` on the live fleet**; live attach run owed before "done."

## The field report

`gr` on the daily-driver looked wrong: **two rows for almost every repo**. A `local-only` working
copy *and* a same-named `home-only` bare repo, ~33 times over. `gr sync` skipped all of them —
*"no configured home remote."* Onboarding "wasn't working": nothing the operator tried made the
duplicates collapse.

Empirically (this is what unlocked it):

- `acer` (primary) and `tenx` (backup) each already hold **all 40 homes, with real content** —
  e.g. `authentik.git = 660a71e`, `omarchy-setup.git = 6f10098`, matching the local HEADs exactly.
  The homes were seeded *from* these working copies. Seeding worked.
- But **~33 local working copies have no `data`/`data-lan` remote** — only a cloud/work `origin`.
  Just 7 (`branching-video`, the 3 `cmecf_*`, `git-redundancy`, `home-fleet`, `USCourts_setup`)
  were wired and show `linked`.

So the two rows were ADR-0019 being *honest*: an unwired local is `LocalOnly` (no backup claim on a
name match), and the bare home is its own `home-only` row. Correct — but no verb joined them.

## Root cause

Every verb dead-ended, and the classifier was the reason:

```rust
// onboard candidate loop — read only the backup:
let kind = if backup_homes.contains(name) { Repoint } else { Create };
```

It **never asked whether a primary home already existed.** With homes on both boxes, these repos
were labelled `Repoint` and routed to a command that then bailed *"no configured backup remote to
compare against"* (repoint's gate compares through a `data` remote that doesn't exist). `create`
refused (*"already exists"*), `sync` skipped (no remote). Confirmed live:

```
$ gr repoint authentik --dry-run
Error: `authentik` has no configured backup remote to compare against
```

Not operator error — a missing case in the classifier.

## The fix (ADR-0020)

A third action, **`attach`**, and a classifier that reads **both** bits:

| primary | backup | action |
|---|---|---|
| no  | no  | `Create` |
| no  | yes | `Repoint` |
| yes | *   | **`Attach`** — the primary is decisive |

- **Pure core:** `core/onboard.rs::classify_onboard(primary, backup) -> OnboardAction`, total,
  exhaustively unit-tested (the "primary present ⇒ attach" case is the regression). The shell reads
  the primary bit from the survey it already has (any `Linked`/`HomeOnly` presence).
- **`attach` (shell):** wire `data`/`data-lan` at the existing primary (`remote_wiring` +
  `wire_and_refresh`), then reconcile both ways with `sync_repo` — push ahead / ff behind / report
  diverged, never force. No home created or seeded. Backup completeness stays cheap: trust a
  live-fleet primary (backup present ⇒ replicating), and only run the shared, idempotent
  `ensure_topology` when the backup is actually missing.
- **UX:** `y` covers attach (same intent as create); `r` still means repoint. `--dry-run` names it.
- **Refactor:** `create`'s topology block extracted to `ensure_topology`, now shared by `create`
  and `attach`'s missing-backup corner. `create`'s behaviour unchanged.

## Verification

- `cargo fmt --check` clean · `cargo clippy --all-targets` clean · `cargo test` 71 green
  (incl. the new `onboard::classify_*` cases).
- `gr onboard --dry-run` on the live fleet: the ~33 formerly-stuck repos now read
  *"would attach → wire local remotes to the existing primary + reconcile"*; the genuinely
  home-less ones (`ecf-data`, `get-hearings`, `mix-a-hoot-n-hollar`, `spaces-game-data`) still fall
  to `create`/BLOCKED (detached-HEAD / no-commits) as before.

## Owed before "done"

A **live attach run** (start with one repo — e.g. `authentik` — confirm it collapses to a single
`linked` row and the backup reads present), then the rest of the walk. After that, the
`git-redundancy` working copy itself is still wired to `tenx` (pre-flip) rather than `acer` — a
separate repoint-style cleanup.
