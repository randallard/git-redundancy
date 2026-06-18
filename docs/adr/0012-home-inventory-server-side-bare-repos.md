# ADR-0012: Home inventory — discover the bare "home" repos on tenx, not just local working copies
- Status: Accepted
- Date: 2026-06-18
- Deciders: Ryan

## Context
Through ADR-0011, `gr` only knows about **local** working copies: discovery scans the
configured `roots` for directories containing a `.git`, and `status`/`push` compare each
local branch to its *remote-tracking* refs. That is blind to the other half of the system —
the **bare "home" repos** at `/data/git/*.git` on `tenx-rltec` (ADR-0009). Three real
situations are invisible today:

1. **local-only** — a working copy here has no bare home yet (nothing backs it up).
2. **home-only** — a bare home exists on tenx that was never cloned to this machine.
3. **drift in branches you don't have checked out** — a home branch you have no local
   copy of, or a local branch never pushed, never shows up because there's no
   remote-tracking ref to compare against.

To surface and later act on these (ADR-0013 lifecycle commands, ADR-0014 status UX), `gr`
needs a first-class notion of the **home** as a presence it can enumerate, independent of
whether a local clone exists. That is a new capability with three non-obvious forces:

- **It is a network read.** Today `status` is local-only and fast and never fails. Querying
  tenx adds latency and a new failure mode (host asleep/unreachable — the orthogonal risk
  ADR-0009 already flagged). Reads must degrade, not break.
- **Identity is not the directory name.** The local dir and the home repo name can differ
  (`~/Development/USCourts_setup` ↔ `/data/git/omarchy-setup.git`). Joining local to home by
  directory name is wrong.
- **Two remotes, one home.** `data` (Tailscale) and `data-lan` (LAN) are interchangeable
  transports to the *same* bare repo (same `HostKeyAlias`, same server path, ADR-0009).
  Inventory must not double-count them.

## Decision
Give `gr` a **home inventory**: the set of bare repos under a configured server root,
queried over the existing SSH transport, joined to local working copies by **home name**.

### 1. A repo is a *name with up to two presences*
- **local** — a working copy discovered under a configured `root` (unchanged from ADR-0011).
- **home** — a bare repo `<root>/<name>.git` on tenx.

Joining the two yields a **lifecycle state** consumed downstream:
`local-only` · `home-only` · `linked` (both present).

### 2. Identity = home name, derived not configured
- For a **local** repo, the home name is the basename (minus `.git`) of its `data` /
  `data-lan` remote URL — so `omarchy-setup` is recovered from the remote even though the
  directory is `USCourts_setup`. No per-repo mapping is stored.
- For **`create`** (ADR-0013), the home name defaults to the local directory name and is
  overridable.
- For a **home-only** repo there is no local remote to read, so its identity simply *is*
  the home name from the server listing.

### 3. `[server]` config block — required for inventory
Home-only repos have no local remote to derive connection details from, so the server
coordinates live in config, not only in git remotes:

```toml
[server]
root = "/data/git"        # where the bare repos live on tenx
# Connection reuses the ADR-0009 transport aliases. By default inventory is queried
# over `transport.order` (try tenx-lan, then tenx-ts); override here if needed.
# aliases = ["tenx-lan", "tenx-ts"]
```

Enumerating the `*.git` children of `[server].root` is the server-side analog of scanning a
local `root` — so this stays **config-first** (ADR / PROGRESS §5): `gr` still acts only on
roots you declared, one of which now lives on tenx. No implicit global scan.

### 4. How inventory is gathered
- One `ssh <alias> 'ls -d <root>/*.git'` to list homes (alias chosen by the ADR-0009
  failover order: LAN first, Tailscale fallback).
- Per home, `git ls-remote <url>` for its branch refs — enough to compute per-branch sync
  (including branches with no local copy) without a working tree.
- `data` and `data-lan` are **deduplicated to one home** by server path, never shown twice.

### 5. Reads degrade; writes do not
- **`status`** queries inventory by default when `[server]` is configured, under a short
  timeout. If tenx is unreachable it falls back to the **local-only** view, marks the home
  columns unknown (`?`) with a one-line note, and **exits 0** — unreachability is a
  documented state, not an error.
- **`--offline`** skips the network entirely for a fast, purely-local view.
- **Lifecycle commands** (`create`/`clone`/`sync`, ADR-0013) *require* the server and fail
  loudly (non-zero) if it is unreachable — they never half-act.

### 6. Scope boundary
Inventory covers **only** the tenx bare-repo home(s) under `[server].root`. The DCN cloud
`origin` is out of scope for inventory and lifecycle actions; `status` may still display an
existing `origin` column read-only as it does today, but `gr` neither enumerates nor
reconciles cloud remotes in this increment.

## Consequences
- `gr` gains a true two-sided picture, unlocking the `local-only` / `home-only` / `linked`
  lifecycle that ADR-0013 acts on and ADR-0014 renders — including drift on branches not
  checked out locally.
- `status` is no longer guaranteed local-only/offline-safe *by default*; the `--offline`
  flag and graceful degradation preserve the old fast path and keep a sleeping tenx from
  turning a status check into a failure.
- The home-name-from-remote rule removes the `USCourts_setup ↔ omarchy-setup` foot-gun
  without a hand-maintained mapping, at the cost of depending on the `data`/`data-lan`
  remote URLs being well-formed (they are, by ADR-0009 / `create`).
- Adds a `[server]` config section and a per-home `git ls-remote` round-trip. For a handful
  of repos this is fine; if inventory grows or feels slow, caching the listing is a future
  option (noted, not built — keeping this increment simple).
- Keeps the functional-core / imperative-shell split (ADR-0002): the SSH/`ls-remote` calls
  are imperative-shell IO; joining presences and classifying lifecycle/branch state stays
  pure `core`, testable without a network.
- Still subject to the ADR-0009 orthogonal risk: if `tenx-rltec` is asleep no addressing
  helps — here it surfaces as the degraded `?` state rather than a crash.
