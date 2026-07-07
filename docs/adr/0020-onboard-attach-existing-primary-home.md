# ADR-0020: `onboard` gains `attach` — a working copy whose primary home already exists
- Status: Accepted
- Date: 2026-06-30
- Deciders: Ryan

## Context
`gr onboard` (ADR-0017) walks the un-redundant repos and, per repo, picks the mechanism that
makes it redundant: `create` (ADR-0016) for a repo with no home anywhere, or `repoint`
(ADR-0018) for the *original-7* sub-state — a home on the **backup** but not the **primary**,
left over from before the fleet flipped (ADR-0015). The walk classifies each candidate into one
of those two.

Dogfooding the real fleet surfaced a **third, unhandled state** that is now the *common* one.
After seeding both boxes, every one of the ~40 repos already had a bare home on the **primary**
(`acer`) **and** the backup (`tenx`) — but ~33 of the local working copies had **no `data`/`data-lan`
remote wired to them** (they carried only a cloud/work `origin`). Per ADR-0019, a working copy
with no home remote is honestly `LocalOnly` — `gr` refuses to claim a backup on a mere
directory-name match — so `gr status` correctly showed *two rows* per repo: the unwired
`local-only` working copy and the same-named `home-only` bare repo. The tool was telling the
truth; there was simply no verb to **join** them.

And every existing verb dead-ended on this state:

- **`create`** refuses — `create_home` bails *"a home named `X` already exists on the server"*
  (lifecycle.rs:82). Correct: it must never clobber a populated home.
- **`repoint`** can't run — its consistency gate compares the local copy to the backup **through
  the local's backup remote**, but there is no `data`/`data-lan` remote to compare through, so it
  bails *"no configured backup remote to compare against"* (lifecycle.rs). Its whole job is to
  provision a **missing** primary, and here the primary is present.
- **`sync`/`push`** skip — *"no configured home remote"* — for the same missing-remote reason.

The root defect was in the classifier itself: it decided `create` vs `repoint` by looking **only
at the backup** —

```rust
let kind = if backup_homes.contains(name) { Repoint } else { Create };
```

— and **never asked whether a primary home already existed**. So these repos were mislabeled
`Repoint` (backup present) and routed to a command guaranteed to fail. That is why "onboarding
hasn't been working": not operator error, a missing case.

The non-obvious forces:

- **The primary is decisive, and it was the unread bit.** Whether a *primary* home exists changes
  the mechanism completely (provision vs. attach), yet the classifier only read the backup.
  Reading both bits makes the decision total.
- **This is wire-and-reconcile, not provision.** The home already holds the content (it was seeded
  from these very copies). The gap is purely local: the working copy isn't connected. So the action
  must **not** create or seed a home — it must wire the local remotes at the existing primary and
  then reconcile drift the safe both-ways way.
- **Onboard promises redundancy, honestly (ADR-0017).** The action must still leave the repo
  two-box redundant, or say plainly that it isn't. On a live fleet the backup is already present and
  replicating, so re-provisioning it per repo is needless SSH churn — but the *missing-backup*
  corner must still be completed.
- **Classification is pure; keep it there (ADR-0002/0019).** The decision is a total function of two
  bits (primary present? backup present?) and belongs in `core`, unit-provable without a network —
  consistent with where `presence`/`sync` classification already lives.

## Decision
Add a third onboarding action, **`attach`**, and make the classifier read **both** the primary and
the backup.

### 1. A pure classifier in `core` (`core/onboard.rs`)
`classify_onboard(primary_home: bool, backup_home: bool) -> OnboardAction`, total over the four
inputs:

| primary home | backup home | action |
|---|---|---|
| no  | no  | `Create` — no home anywhere; provision the full topology (ADR-0016) |
| no  | yes | `Repoint` — backup-only; provision the primary, re-role the backup (ADR-0018) |
| yes | *   | **`Attach`** — the primary home exists; wire the local copy to it and reconcile |

**The primary is decisive:** if it exists, the answer is always `Attach`, regardless of the backup
— `create` would refuse and `repoint` has no missing primary to provision. The shell derives the
two bits from the survey it already has: the backup listing (ADR-0015) and the primary homes
(any `Linked`/`HomeOnly` presence means the primary home is present, ADR-0012).

### 2. The `attach` action (shell)
For a candidate whose primary home exists:
1. **Wire** `data`/`data-lan` at the existing primary via the shared `remote_wiring` +
   `wire_and_refresh` (set-url/add then `fetch --prune`, ADR-0009/0019) — no home is created or
   seeded.
2. **Reconcile** both ways through the freshly-wired remotes with the *same* easy-sync machinery
   `gr sync` uses (`sync_repo`): push ahead, fast-forward behind on a clean tree, report diverged —
   **never force, never auto-merge** (ADR-0006). All branches (`-a`), matching onboard's
   backup-completeness rule.
3. **Backup completeness, cheaply:** trust a live-fleet primary (backup present ⇒ already
   replicating) and skip re-provisioning; only when the backup home is **missing** run the shared,
   idempotent `ensure_topology` (post-receive on the primary, backup home created + hardened +
   ff-only pre-receive) — the same block `create` uses, now factored out.

### 3. UX: `y` covers `attach`; `r` still means `repoint`
`attach` is the operator's ordinary "yes, make this redundant," so it is offered under the existing
**`y`** (like `create`) — the classifier, not the operator, picks create-vs-attach. **`r`** remains
reserved for the distinct, heavier `repoint`. `--dry-run` names the attach plan explicitly
(*"would attach → wire local remotes to the existing primary + reconcile"*). The detached-HEAD /
no-commits pre-flight and the `ignore` list are unchanged.

### Carried over, unchanged
- `attach` introduces **no new server-side mutation mechanics** — wiring is ADR-0009/0019, the
  reconcile is ADR-0013 `sync`, the optional backup completion is ADR-0016 `create`'s own block.
  It is a **composition**, audited (`attached`) and fail-loud like the rest.
- ADR-0019 stays intact: attach exists precisely *because* an unwired local is honestly
  `LocalOnly`, never a false `linked`.

## Consequences
- **`onboard` handles the whole catalog.** The dead-end is gone: the ~33 seeded-but-unwired repos
  now classify as `attach` and, on `y`, collapse from two rows to one `linked` row with the backup
  present — the fleet becomes actually (not just nominally) redundant.
- **The classifier is total and provable.** The create/repoint/attach decision is a pure
  `core` function with exhaustive unit tests over the four-cell table; the "primary present ⇒
  attach" case is the regression the old code got wrong. No network needed to test the decision.
- **`create`'s topology block is now shared (`ensure_topology`).** One source of truth for the
  post-receive + hardened-backup provisioning, reused by `create` and by `attach`'s missing-backup
  corner. `create`'s behaviour is unchanged (the extraction is mechanical).
- **Attach trusts a live fleet for speed.** Skipping re-provisioning when the backup is already
  present avoids dozens of redundant SSH round-trips; the cost is an assumption ("backup present ⇒
  replication healthy") that holds for gr-provisioned homes and is completed for the missing-backup
  case. A primary home created out-of-band without its post-receive hook would attach but not
  replicate until its next `create`/repoint — an accepted, documented edge.
- **A fourth verb was *not* added.** Attach lives inside `onboard` (and reuses `sync`), keeping the
  command surface small; there is deliberately no standalone `gr attach`. If a direct
  `attach <name>` is ever wanted, it earns its own ADR.
