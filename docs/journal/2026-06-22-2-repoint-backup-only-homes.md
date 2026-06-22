# 2026-06-22 (2) — Repoint: backup-only homes into the current topology (ADR-0018)

**Documents:** commit `09da50f` ("feat(repoint): bring backup-only homes into the current
topology"). The ADR landed earlier in `66d56c9` (alongside ADR-0017); this is the
implementation. Second entry today — follows the
[onboard walk](2026-06-22-onboard-guided-walk.md), whose `r` stub this fills in.
**Status:** `gr repoint` is built, tested, and green. The `gr onboard` walk's `r` now calls it
instead of printing "not yet implemented," so onboarding can resolve **every** un-redundant
kind. The original 7 finally have a path — pending a live round-trip against the real fleet.

## What landed

The repoint operation closes the last lifecycle gap: a repo whose home is on the **backup** but
not the **primary** (the original-7 sub-state, from before the fleet flipped to acer-primary /
tenx-backup). It's *not* `create` — the backup already holds real history, so the whole job is
making the primary authoritative **without losing what the backup has**.

- **`gr repoint <name>`** — a first-class lifecycle verb (parity with create/clone/sync), also
  the walk's `r`. Resolves the repo via the `status` survey and confirms the backup-only
  sub-state (local-only against `[server]`, home present on `[backup]`).
- **`repoint_repo`** (shared by the verb and onboard) — the ADR-0018 two-phase shape:
  - **Consistency gate** (pure-ish, before any mutation): each branch classified against the
    backup with the existing `branch_sync`. `ahead` / `equal` / `new-local-branch` → proceed;
    `behind` → refuse and send to `gr sync`; `diverged` → refuse. Never force, never auto-merge.
  - **Ordered, rewire-last flip:** ① provision the primary (`init_bare` + `set_head` +
    `post-receive`, idempotent) → ② **seed** it from the verified-superset local copy, pushed
    **by URL** so `data`/`data-lan` stay on the backup → ③ **re-role** the existing backup home
    (`harden_home` + `pre-receive`, and drop the stale `post-receive` from its primary days) →
    ④ **confirm** the backup fast-forwards from the new primary → ⑤ **repoint** the client's
    remotes at the primary, *last*.
- **New primitives:** `git::ls_remote_sha` + `git::is_ancestor` (the step-④ ff-consistency
  check, over local objects), and `server::remove_hook` (step ③'s stale-hook drop). Audited as
  `repointed`; `--dry-run` previews the plan and the per-branch gate result.

## How the decisions showed up

- **Never lose history, twice (CP/SI).** The pre-mutation gate refuses any branch where the
  local copy isn't ahead-or-equal of the backup; and the backup's standing ff-only guard
  (`pre-receive` + `denyNonFastForwards`/`denyDeletes`, ADR-0016) is the server-side backstop, so
  a mirror can only ever *advance* the backup. Step ④ re-checks for the TOCTOU window.
- **Rewire-last makes failure cheap (ADR-0012 §5).** Seeding pushes to the primary's URL, not a
  wired remote; the client's `data`/`data-lan` move only after the primary is built, seeded, and
  confirmed. Every abort path prints that the remotes are untouched and the data is safe.
- **Reuse over rebuild (ADR-0002).** The gate is the `sync`/`status` classification; the flip is
  ADR-0016's existing `io::server` primitives parameterized by alias, plus three small additions.
  No new mutation mechanics on the backup beyond removing one hook.
- **Trust direction untouched (AC).** repoint provisions the primary and hardens the backup from
  the client's per-host keys; it never touches the primary→backup forced-command replication key.

## Verification

- **Hermetic** (no SSH): the `gr repoint` guards — no `[server]` (lifecycle-disabled) and no
  `[backup]` (refused before any network). **67 tests** (core 21 · io 19 · cli 27);
  `fmt` (my code) + `clippy -D warnings` clean; the 3 Kani proofs untouched.
- **Not yet:** a live round-trip. The gate and the five-step flip only run against a real
  primary + backup, so they're verified by construction and review here, not in CI.

## Honest debt

- **Step ④ confirms safety, not completeness.** The primary→backup mirror fires **async** from
  the primary's `post-receive` (the controller's job, under its own key), so the client can't see
  the mirror land. The check verifies the backup isn't *divergent* from the primary (which the
  gate already guarantees); it does **not** prove the mirror finished catching up — the
  controller sweep does that. So repoint reports "fast-forward-consistent," not "fully mirrored."
- **Live validation owed before the original 7.** Recommended path: `gr repoint --dry-run` to
  read the gate, then one throwaway repo end-to-end, before running it on anything that matters.
- **Pre-existing `io/server.rs` fmt drift** still present (two lines my local rustfmt rewraps,
  from before this work) — left untouched to keep the changeset scoped; worth reconciling
  separately with whatever rustfmt CI pins.

## Next

- A **live round-trip** on a throwaway backup-only repo, then onboard the original 7 via the
  walk's `r`. After that, the lifecycle surface (create / clone / sync / onboard / repoint) is
  feature-complete for the two-box promise; remaining work is assurance (live-path coverage) and
  the parked backup *sync-state* surfacing (ADR-0016's follow-up).

## Postscript — CI after the push (commit `74f6833`)

The push went red. Two jobs failed, and they're worth recording because the second is a real
decision, not a slip:

- **fmt** — `cargo fmt --all --check` flagged `io/server.rs:62`. This was the **pre-existing
  drift** the previous two entries called out as "left untouched": CI's rustfmt agrees with my
  local one, so it was never cosmetic — it had been failing since the **ADR-0016 push
  (`36e3c21`)**. My caution about not reformatting a file I didn't "own" was the wrong call; the
  committed code was simply not canonical. **Fixed** by running `cargo fmt` (the two rewrapped
  lines + the `remove_hook` addition are now clean).
- **coverage gate** — line coverage **59.95%** against the `--fail-under-lines 70` floor.
  onboard + repoint roughly doubled the SSH-orchestration surface (`server.rs` ~50%, `git.rs`
  ~56%, `lifecycle.rs`), none of it hermetically testable without a mock — the exact
  "verified by hand" caveat the gate's own comment already carries. **This is the open
  deliberation** (see PROGRESS "Where we are"): lower the floor to ~58%, *exclude* the
  network-shell files and hold a high bar on the testable core, or build an SSH stub to claw
  coverage back. Parked here deliberately — it's a quality-bar call, not a code fix, and it's
  next on the plate. (kani proofs + supply-chain jobs stayed green.)
