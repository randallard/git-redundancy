# 2026-06-22 (4) — A false "up-to-date": stale tracking refs after a remote repoint

**Documents:** no code commit yet — this is a **discovery + remediation plan**, landed ahead of
the fix (next entry will reference the work commits). Fourth entry today; follows the
[repoint](2026-06-22-2-repoint-backup-only-homes.md) and [CI-greening](2026-06-22-3-greening-ci-coverage-debt.md)
entries.
**Status:** ⚠️ **correctness bug found in `gr push` and `gr status`.** A real backup was silently
skipped while `gr push` reported `up-to-date`. Root cause understood; fix scoped into four parts.

## What happened (the field report)

Dogfooding the fleet flip (clients moving from a tenx-primary world to acer-primary /
tenx-backup), several working copies had their `data` / `data-lan` remotes repointed from the old
home server to the new one with plain `git remote set-url` (the SETUP.md §4 move). For one repo:

- `git remote set-url data-lan ssh://acer-lan/data/git/<repo>.git` (and `data` → `acer-ts`)
- `gr status <repo>` → `data-lan: ok`, `sync: ok`
- `gr push --only <repo>` → **`up-to-date`, 0 pushed**
- but the **primary home on acer was one commit behind** the working copy. The push that should
  have happened never fired. `gr` asserted "backed up" when it was not.

Verifying the actual server ref (`git --git-dir=… rev-parse main`) on both boxes is what caught
it: acer at the old commit, the working copy (and the *backup*, tenx) one ahead.

## Root cause

`io/git.rs::ahead_behind` (git.rs:96) is, by deliberate design, **network-free** — it counts
`rev-list --left-right --count <branch>...<remote>/<branch>` against the **local remote-tracking
ref**. Its own doc comment says so: *"using the local remote-tracking ref (no network)."*

`git remote set-url` repoints the remote **but does not touch the tracking ref**. So
`refs/remotes/data-lan/<branch>` keeps the value last fetched from the *old* server. After a
repoint it equals the working copy (the old server had the work), so:

- `branch_sync` → `ahead 0, behind 0` → **`UpToDate`**
- `gr push` (push.rs:175) sees `UpToDate`, prints `up-to-date`, and **never attempts the push**
- `gr status` (main.rs:411/462/556) renders a confident **`ok`**

Both read a stale ref and report it as truth. The one command that gets this right is **`gr sync`**,
which fetches first (lifecycle.rs:997, `if git::fetch(repo, r)?.success`) before classifying —
proving the fetch-then-classify pattern is already in the codebase, just not on the `status`/`push`
paths. A `git::fetch` helper already exists (git.rs:214) and is documented *"refresh remote-tracking
refs before classifying"* — it simply isn't called from where it's most needed.

## Why this is the worst failure mode

`gr`'s entire promise is the end-of-day question: *"is all my work safely backed up?"* A false
`up-to-date` answers "yes" when the answer is "no." Every other failure mode is conservative by
design (skip-and-report, never force); this one **under-reports risk**, which is the one direction
the tool must never err.

## The fixes (four parts, in priority order)

1. **[safety] `gr push` must not trust a stale ref.** Before classifying a branch for push, fetch
   the remote it is about to act on (over the same transport-failover order it would push through),
   exactly as `sync` already does. Equivalent fallback: attempt the push and let git's own
   fast-forward check decide — but that discards the `diverged`/`behind` classification `gr` wants
   for its messaging, so **fetch-then-classify** is preferred. *(push.rs)*

2. **[correctness] `gr status` shows stale ahead/behind after a repoint.** Same root cause; this is
   what manufactured the false confidence that hid #1. `status` already does network (the server
   inventory), so "local-only columns" is not a property worth protecting at the cost of being
   wrong. Plan: **fetch before computing the per-remote columns by default**, keep a `--offline`
   flag for the fast, explicitly-stale path. (Cheaper alternative if fetch-by-default is rejected:
   detect that a remote's configured URL no longer matches the source of its tracking ref and render
   a `stale`/`?` marker instead of `ok`.) *(main.rs)* — decided in
   [ADR-0019](../adr/0019-freshness-before-classification-status-push-fetch.md), since it changes
   `status` from local-by-default to network-by-default.

3. **[root-cause containment] a remote repoint must refresh its tracking ref.** Every place that
   calls `set_remote_url` leaves the ref stale: `gr repoint` step 5 (lifecycle.rs:765–772),
   `clone`/`create` wiring (lifecycle.rs:161, 819), and the manual SETUP.md §4 loop. Wrap "set-url
   then `git fetch <remote>` (`--prune`)" so a repoint leaves a *truthful* ref behind, and document
   the `git fetch` step for the manual path. This makes #1/#2 belt-and-suspenders rather than the
   only line of defense. *(git.rs + lifecycle.rs + SETUP.md)*

4. **[classification] `linked` / `Bkp` can be a name coincidence, not a real link.**
   `core/presence.rs::join_presences` (presence.rs:66) classifies a local repo `Linked` when its
   *effective* home name matches a server home — but for a repo with **no `data` remote** the
   effective name falls back to the **directory name** (presence.rs:71). So a repo wired only to a
   cloud `origin` (no fleet remote at all) shows `linked` + `Bkp ok` purely because its directory
   name equals an existing home — a false "backed up" signal for a repo that isn't wired to that home
   at all. Fix: a local repo with `home_name == None` must never be `Linked` (it is `LocalOnly`);
   reserve `Linked` for repos whose **remote actually resolves** to a server home. Optionally surface
   a "dir name collides with a home you're not wired to" hint. *(core/presence.rs — pure, fully
   unit-testable; note the existing tests at presence.rs:133/164 encode the current, wrong behavior
   and must be updated.)*

## Immediate operational workaround (until the fix lands)

After any manual `git remote set-url`, **`git fetch <remote>` before trusting `gr status`/`gr push`**.
The fetch updates the tracking ref to the new server's real state, after which classification is
correct. Captured in [TROUBLESHOOTING](../TROUBLESHOOTING.md).

## Verification owed (when the fix lands)

- A hermetic test that repoints a remote to a server that is **behind** the working copy and asserts
  `gr push` actually pushes (not `up-to-date`) — the exact regression. (#1)
- `presence.rs` unit test: a local repo with `home_name == None` whose dir name matches a home is
  `LocalOnly`, not `Linked`. (#4)
- Re-confirm the dogfooded fleet by reading **server refs**, not `gr status`, until #1/#2 ship.
