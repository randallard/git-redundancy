# 2026-06-18 — Lifecycle commands: create / clone / sync (ADR-0013)

**Documents:** commit `65ef6ff` ("ADRR-0013"). Follows `2362707` (journal for the home
inventory) and `0c52f6c` (the inventory itself). Second entry today — see the
[inventory/course-correct entry](2026-06-18-home-inventory-and-a-course-correct.md) for the
ADR-0012 foundation this builds on.
**Status:** `gr` can now *close* the local↔home gap, not just see it — `create`, `clone`, and
`sync` all work and were exercised live against tenx. The status UX that ties it together
(ADR-0014) is the last piece.

## What landed

Three mutating, networked verbs, built on the inventory + push-safety layers, keeping the
ADR-0002 functional-core / imperative-shell split:

- **`core::sync`** (pure) — `SyncAction::plan(sync, tree_clean)` collapses a branch's
  classification into one safe action: `Push` (ahead/new), `FastForward` (behind, clean tree),
  `BlockedDirty` (behind, dirty), `Report` (diverged/CONFLICT). Unit + a proptest that a
  fast-forward is *only ever* planned for a clean tree. **100% covered.**
- **`io::server`** (shell) — server-side ops over the ADR-0009 transport: `init --bare`,
  `set HEAD`, existence check, `pick_alias` (first reachable), and `remote_wiring` (pairs
  `transport.order` with the SSH aliases positionally: `data-lan`↔`tenx-lan`, `data`↔`tenx-ts`).
- **`io::git`** — local mutations: `clone`, add/set/remove-remote, `fetch`, `ff_merge_current`
  (current branch, `merge --ff-only`), `ff_update_branch` (non-current, `fetch . src:dst` —
  ff-only, no working-tree touch).
- **`gr create [name]`** — make the bare home, set its `HEAD` to the pushed branch, wire both
  remotes, push; refuses if a home already exists (points to `sync`).
- **`gr clone <name> [dir]`** — clone a home-only repo; the target **must land inside a
  configured root** (else it lists the roots + how to add one and stops); drops the
  clone-minted `origin` so it stays cloud-only.
- **`gr sync [repos…]`** — reconcile easy work both ways; `-i` confirms each effecting action,
  `-a` runs all branches, `--dry-run` previews. All three audit their actions.

## How the decisions showed up

- **ADR-0013 safety** — the one new mutation class, the fast-forward *pull*, is gated exactly
  as written: only the current branch's ff touches the working tree (so it's gated on a clean
  tree); non-current branches fast-forward via a refspec fetch that can't touch the tree at
  all. Diverged/CONFLICT are reported, never forced or auto-merged. ADR-0006's spirit holds.
- **ADR-0009** — `create`/`clone` wire `data`/`data-lan` to their aliases; `sync` reuses the
  LAN→Tailscale failover (fetch each candidate until one answers = the live transport).
- **ADR-0012** — identity stays the home name; `create` defaults it to the directory name;
  the server coordinates come from `[server]`.
- **ADR-0004 (AU)** — create/clone/sync all append audit records; `--dry-run` performs nothing
  and is deliberately not audited.

## Verification

- **Live round-trip against tenx** (throwaway `gr_lifecycle_selftest`, auto-cleaned): `create`
  → server has the commit, both remotes resolve correctly → `sync` reports up-to-date →
  `clone` into a second dir round-trips with `origin` dropped.
- **Hermetic** (file-path remotes, no SSH): `sync` push / dry-run / fast-forward / dirty-block,
  plus the non-network branches (clone-target refusal, server-required guard, no-match filter).
- **Gates:** `fmt` + `clippy -D warnings` clean; **52 tests** (core 21 · io 14 · cli 17);
  the 3 Kani integer proofs untouched.

## Honest debt

- **Coverage dipped to ~76% line** (was 84%). The pure core is 98–100% (`sync.rs` 100%), but
  `io::server` (~65%) and `create`/`clone`'s orchestration in `lifecycle.rs` (~54%) have
  SSH-execution paths that only run against a live server — verified by hand, not in CI. Same
  inherent limitation as the rest of the network code; an SSH stub/mock could close it later.
- **`gr homes` still standing** alongside the new verbs — its fate (retire vs. stay) is the
  ADR-0014 question, settled next.

## Next

- Implement **ADR-0014** — fold inventory + lifecycle into `gr status`: a lifecycle cell,
  `home-only` rows, the `+N⚠` "others need attention" indicator, and a positional
  `gr status <repo>` detail that previews what `sync` would do. At that point `gr homes`
  either retires or becomes a thin alias.
