# ADR-0019: Freshness before classification — `status`/`push` fetch before computing ahead/behind
- Status: Accepted
- Date: 2026-06-22
- Deciders: Ryan
- Verified-by: `cli test: repoint_to_behind_home_still_pushes` — the regression for the dangerous
  under-reporting case (a repointed remote reading a false `up-to-date`). **Live round-trip
  against server refs still owed.**

## Context
ADR-0012 built the home-aware status on a deliberately **network-free** ahead/behind computation:
`io/git.rs::ahead_behind` counts `git rev-list --left-right --count <branch>...<remote>/<branch>`
against the **local remote-tracking ref** — fast, no SSH round-trip per branch, and at the time the
remotes were stable so the local ref was a faithful proxy for the server. `gr status` reads it for
the per-remote columns; `gr push` reads it (via `branch_sync`) to decide whether a branch is
`UpToDate` and skip it. `gr sync` is the lone exception: it `git fetch`es first (lifecycle.rs:997)
before classifying.

Dogfooding the fleet flip (clients moving from a tenx-primary world to acer-primary / tenx-backup,
ADR-0015/0016/0018) broke that assumption and exposed a **silent correctness bug** — written up in
the [journal](../journal/2026-06-22-4-stale-tracking-refs-after-repoint.md):

- A working copy's `data`/`data-lan` remote was repointed at the new home server with
  `git remote set-url` (the SETUP.md §4 move, and the same call `gr repoint` makes internally at
  lifecycle.rs:765–772).
- **`git remote set-url` does not update the remote-tracking ref.** It still held the *old* server's
  value — which equalled the working copy, because the old server had the work.
- So `ahead_behind` returned `0/0` → `branch_sync` → `UpToDate`; `gr push` printed **`up-to-date`
  and never attempted the push**, and `gr status` rendered a confident **`ok`** — while the new
  primary home was a commit **behind**. The backup that should have happened didn't.

The non-obvious forces:

- **This is the one disallowed direction of error.** Every other `gr` failure mode is conservative
  by construction — skip-and-report, never force, never auto-merge (ADR-0006). This one
  **under-reports risk**: it answers the tool's whole reason for existing ("is my work safely backed
  up?") with a false *yes*. Speed is not worth being wrong in that direction.
- **The local-ref-is-truth premise is exactly what a repoint violates.** The premise held while
  remotes were static. The moment a remote's URL changes — onboarding, a fleet flip, a transport
  swap — the local ref describes a server you no longer push to. Repoint is now a first-class verb
  (ADR-0018), so this is a recurring state, not a one-off.
- **`status` is already a network command.** It SSHes the server for the home inventory (ADR-0012)
  and the `[backup]` presence (ADR-0015). "Per-remote columns are computed locally" is an internal
  optimization, not a user-visible contract worth protecting at the cost of correctness.
- **The fix already exists in-tree, on one path.** `git::fetch` exists (git.rs:214) and is
  documented *"refresh remote-tracking refs before classifying"*; `sync` already calls it. The
  defect is that the most safety-critical path (`push`) and the most-read path (`status`) don't.
- **A related over-statement lives in the pure join.** `core/presence.rs::join_presences` marks a
  local repo `Linked` when its *effective* home name matches a server home, but for a repo with **no
  `data` remote** the effective name falls back to the **directory name** (presence.rs:71). A repo
  wired only to a cloud `origin` then shows `linked` + `Bkp ok` purely on a name collision — a
  second flavour of "status over-states backup safety," in the pure core this time.

## Decision
Adopt **freshness before classification**: any command whose output asserts backup state must
reflect the *server's* state, not a possibly-stale local proxy. Concretely:

1. **`gr push` fetches the target remote before classifying** (safety-critical). Before deciding a
   branch is `UpToDate`, fetch the remote it is about to act on, over the same transport-failover
   order the push would use, exactly as `sync` already does. A fetch failure is reported, not
   silently treated as "up-to-date." (Defense in depth: even misclassified, a plain `git push` only
   fast-forwards — but `push` must not *skip* on stale data, which no git-side guard can catch.)
2. **`gr status` fetches before the per-remote columns, by default, with `--offline` to opt out.**
   Network-by-default makes the columns truthful; `--offline` keeps the fast, explicitly-stale local
   path for when the server is unreachable or speed matters (and already exists to skip the server
   query — its meaning extends cleanly to "skip the ref-refresh fetch too," rendering affected cells
   as `?`/`stale` rather than a confident `ok`).
3. **A repoint refreshes its own ref** (root-cause containment). Wherever `set_remote_url` runs —
   `gr repoint` step 5, `clone`/`create` wiring — follow it with `git fetch --prune <remote>` so the
   ref is truthful the instant the URL changes, independent of (1)/(2). The manual SETUP.md §4 flow
   gains the same documented `git fetch` step.
4. **`Linked` requires a resolved home remote, not a name match** (pure-core correctness). In
   `join_presences`, a local repo with `home_name == None` is **never** `Linked` — it is `LocalOnly`.
   `Linked`/`Bkp` are reserved for repos whose remote actually resolves to a server home. A
   directory-name collision with an existing home may be surfaced as a hint, but never as a backup
   claim.

Carried-over properties: still **fail-loud, never-force, audited** (ADR-0006/0012 §5). The
network-free `ahead_behind` primitive is unchanged — it is correct *given a fresh ref*; this ADR
fixes *who refreshes the ref and when*, not the counting.

## Consequences
- **The tool stops lying in the one direction it must not.** A false `up-to-date` after a repoint
  becomes a real push (or a loud fetch failure). This is the headline fix and the reason the ADR
  exists.
- **`gr status` and `gr push` become network operations on the per-remote columns.** `status`
  already SSHes for inventory, so the marginal cost is per-remote fetches; `--offline` remains the
  escape hatch and now also means "trust the local ref, mark unknowns." Worth measuring on a large
  fleet — if per-branch fetch latency bites, a future optimization is a single `git fetch <remote>`
  per remote (refreshing all branches at once) rather than per-branch, or `ls-remote` for a
  read-only compare without writing refs.
- **Repoint is self-correcting.** Containment (3) means even code paths that bypass (1)/(2) — or a
  user on an older build — leave a truthful ref behind, shrinking the bug's blast radius to "a
  manual `set-url` with no follow-up fetch," which the docs now cover.
- **`presence.rs` gets stricter and its tests change.** The existing tests encode the current,
  wrong behavior (a dir-name match → `Linked`) and must be updated; the join stays pure and
  fully unit-testable, so the fix is cheap and provable. Net: fewer false `linked`/`Bkp ok` rows.
- **A regression test is owed and now specifiable:** repoint a remote to a server that is *behind*
  the working copy and assert `gr push` actually pushes (not `up-to-date`) — the exact field
  failure — plus a `presence.rs` unit test that a `home_name == None` repo is `LocalOnly` even when
  its dir name matches a home. Both are listed in the journal's "verification owed."
- **`--offline` semantics are now load-bearing**, not just a server-query skip. It must clearly
  render affected cells as unknown (`?`/`stale`) so "fast" never reads as "verified."
