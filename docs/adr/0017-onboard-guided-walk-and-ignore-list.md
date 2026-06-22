# ADR-0017: `gr onboard` — a guided per-repo walk, with a config `ignore` list
- Status: Accepted
- Date: 2026-06-22
- Deciders: Ryan

## Context
ADR-0016 made a single `gr create` yield real redundancy: primary home + replication hook +
hardened backup home. ADR-0014 made the gap *visible* — the fleet view lists every repo with its
lifecycle (`local-only` · `home-only` · `linked`) and a `+N⚠` hint. What's missing is the
**connective verb between seeing and doing**: a way to go down the catalog of not-yet-redundant
repos and decide each one — *onboard this, ignore that, come back to the other* — without either
hand-running `create` per repo or batch-running it across the whole list.

The catalog as it stands has three kinds of un-redundant repo, and they don't want the same answer:

1. **`local-only`** working copies with no home yet — the main onboarding target (→ ADR-0016
   `create`).
2. **Backup-only homes** — the *original 7* set up when **tenx** was primary. The fleet has since
   flipped (ADR-0015/0016: **acer** is primary, **tenx** is backup), so these have a home on the
   *backup* but none on the *primary*, with replication pointed the wrong way. They need
   **repointing** into the current topology, not a fresh `create`.
3. Repos that are deliberately **not worth backing up** (scratch, vendored, throwaway) — which
   today have no home for the same `+N⚠` reason as a real candidate, so the fleet view nags about
   them on every glance.

The non-obvious forces:

- **The decision is per-repo and human — never batch.** Onboarding is a judgment call (is this
  worth two-box redundancy? is it onboard-able as-is?), so the right shape is a guided walk where
  the operator answers one repo at a time, not a sweep that acts on the whole list. This matches
  how this fleet is run.
- **"Not backing this up" is a real, durable state — and must stay honest.** A repo the operator
  chose to leave unprotected is *different* from one not yet reached. Hiding it would quietly drop
  it from the fleet's accounting (the opposite of ADR-0014's "don't let a clean row mask
  un-backed-up work"). The decision must **persist** and stay **visible**.
- **Discovery/classification already exists.** `gr status` (ADR-0012/0014) already enumerates every
  repo and labels its lifecycle. `onboard` should **reuse that**, not re-walk the filesystem —
  it is a driver over the same classification, plus the `create`/repoint side effects.
- **Config is the home for operator intent (ADR-0012, config-first).** The roots, remotes, and
  `[server]`/`[backup]` topology all live in `config.toml`; a hand-editable, travels-with-config
  list is consistent with that, versus a hidden state file.

## Decision
Add **`gr onboard`** — an interactive walk that reuses the `gr status` discovery/classification,
then steps through the **un-redundant, non-ignored** repos one at a time, letting the operator
decide each.

### The walk
For each candidate, show enough to decide well, then prompt:

```
[3/19]  squaredance        12 MB · main · github origin · last commit 2026-05-30
        7 uncommitted, 16 untracked (won't be backed up)
  onboard (y) / ignore (n) / skip (s) / quit (q) ?
```

Context per repo: **size, current branch, origin presence, last-commit date**, and a
**dirty/untracked warning** (uncommitted work is *not* what `create -a` backs up — only committed
branches are — so the warning is load-bearing, not decoration).

Options:
- **`y` — onboard.** Runs the ADR-0016 `create` provisioning **always with `-a`** (all branches),
  for backup completeness — a half-backed-up repo is the exact gap ADR-0014's `+N⚠` exists to
  catch, so onboarding closes it fully or not at all. Result: redundant on both boxes.
- **`n` — ignore.** Records the repo in a persisted **`ignore` list** (below). It never nags again,
  but stays **visible** (see lifecycle, below).
- **`s` — skip for now.** Leave it `local-only`; ask again next run.
- **`q` — quit.** Stop the walk; **every decision made so far is already saved** (each `y`/`n`
  commits as it happens), so the walk is fully **resumable** — the candidate set shrinks each run
  until every repo is either redundant or deliberately ignored.
- **`r` — repoint** *(offered only for the backup-only homes — the original 7)*. Brings a repo
  that has a home on the **backup** but not the **primary** into the current ADR-0016 topology:
  provision + hook the primary home, point replication primary→backup the right way, reusing the
  hardening path rather than a fresh `create`. Offered inline so the walk handles **all** the
  un-redundant kinds, not just `local-only`.

### Pre-flight: un-onboardable repos are flagged, not errored mid-walk
A repo in **detached HEAD** or with **no commits** can't be onboarded as-is. The walk **auto-flags**
these — *"can't onboard as-is — ignore (n) or fix and re-run?"* — instead of erroring partway
through `create`. The walk never leaves a repo half-provisioned (ADR-0016 fail-loud, ADR-0012 §5).

### `--dry-run`
`gr onboard --dry-run` walks and **shows what each repo is and what would happen** (onboard /
repoint / already-ignored) **without prompting or mutating** — a preview first pass over the
catalog. Not audited, consistent with `sync`/`push` dry-runs (ADR-0013).

### The `ignore` list — in `config.toml`
A persisted `ignore = ["repo-a", "repo-b"]` array in `config.toml`:
- **Config, not a side state file** — config-first (ADR-0012), human-editable, travels with the
  rest of the operator's intent. `gr onboard`'s `n` appends to it; the operator can also edit it by
  hand. (CM — the deliberate "don't protect this" choice is a configuration item, recorded as one.)
- **By name, path on collision.** Ignore matches the ADR-0012/0014 **home name** (which already
  doubles as the repo identifier in `status`). If two roots ever hold same-named repos, the
  colliding entry falls back to a **path** to disambiguate — name first because it reads well and is
  what the rest of the UI uses.

### `ignored` is a visible lifecycle state in `gr status`
Ignored repos keep appearing in the fleet view with an **`ignored`** lifecycle (a dim line / footer
count), **not hidden**. You never lose track that the repo exists and is *deliberately
unprotected* — honesty over a clean table, the same value ADR-0014's `+N⚠` encodes. `ignored`
suppresses the `+N⚠` nag (the operator already decided) but not the repo's existence.

### Carried over, unchanged
- `onboard`'s `y`/`r` are exactly ADR-0016 `create`/repoint provisioning — same never-clobber,
  idempotent, fail-loud, audited (AU) behavior; `onboard` is a **driver**, not new mutation
  mechanics.
- Classification, lifecycle vocabulary, and home-name identity are ADR-0012/0014 reused as-is.
- `[backup]` unset ⇒ `y` still warns "not redundant" exactly as ADR-0016; `r` is simply not offered
  (no backup topology to repoint into).

## Consequences
- **The "see it → decide it" loop closes.** ADR-0014 surfaced the gap, ADR-0016 made one repo
  redundant in one verb; `onboard` is the walk that carries the operator across the *whole catalog*
  at their own pace, one decision per repo — and it's resumable, so it survives being interrupted.
- **A fourth lifecycle state, `ignored`, joins the model** (`local-only` · `home-only` · `linked` ·
  `ignored`). It is the first state that means a *chosen non-action*; keeping it visible is what
  stops it from becoming a silent hole in the fleet's two-box accounting (CP).
- **Operator intent now lives in config in two forms** — the topology (`[server]`/`[backup]`) and
  now the `ignore` list. Both hand-editable, both travel together; the cost is one more array
  someone could typo, mitigated by name-matching the same identifier `status` already prints.
- **`onboard` introduces no new server-side mutation** — every write is ADR-0013/0016 `create` or
  the repoint path, already audited and already fail-loud. The new code is the **walk driver** and
  the `ignore` read/append: pure classification + prompt loop in the shell, over the `core` state
  `status` already computes (ADR-0002). The `y`/`r`/`n`/`s`/`q` dispatch and pre-flight flagging are
  testable without a network.
- **`r` (repoint) is specified here at the walk level but its provisioning details lean on
  ADR-0016.** If repointing turns out to need mechanics beyond ADR-0016's create/harden path
  (e.g. relocating an existing backup home, re-aiming the hook), that earns its **own ADR**; this
  one commits only to *offering* `r` in the walk and to the topology it targets (primary
  authoritative, backup receive-only). *(Done: those mechanics are now **ADR-0018**.)*
- **`--dry-run` makes the first pass safe and legible** — the operator can survey the catalog,
  including which repos are flagged un-onboardable, before committing to a single change.
