# ADR-0016: `gr create` provisions the full fleet topology (replication hook + hardened backup home)
- Status: Accepted
- Date: 2026-06-21
- Deciders: Ryan

## Context
ADR-0013 gave `gr create` the `local-only → linked` transition: `git init --bare` a home on the
**primary** (`[server]`), set its `HEAD`, wire `data`/`data-lan`, and push. ADR-0015 then added a
**`[backup]`** server and a `Bkp` presence column so the second half of redundancy — "is the home
*also* on the backup?" — is visible.

Dogfooding the onboarding flow (the companion **home-fleet** project, cutting its own repos onto
the new primary) exposed a gap: **`gr create` makes a repo *present* on the primary, but not
*redundant*.** Two pieces that the original homes received during the companion's one-time
replication setup are missing for a freshly-`create`d home:

1. **No `post-receive` replication hook** on the new primary home. `git init --bare` installs no
   hooks, so a push to the new home does **not** trigger the primary→backup mirror.
2. **No home on the backup** to receive into — and the backup is deliberately locked to
   `git-receive-pack` behind a forced command (home-fleet ADR-0002/0003), so replication
   **cannot auto-create** it. The mirror has no target.

Observed concretely: after `gr create` of `home-fleet`, `Bkp` read `miss`, and the controller
sweep logged `FAIL  home-fleet.git -> tenx-backup :: does not appear to be a git repository` until
a backup home was created by hand. The fleet's core promise — *every repo on two independent
boxes* — is **not** met by `create` alone (CP, contingency planning).

The non-obvious forces:

- **The replication wiring is real but out-of-band.** The hook bodies and the backup hardening
  (`receive.denyNonFastForwards` + `receive.denyDeletes` + a `pre-receive` that rejects deletes
  and non-ff) live as shell in the companion home-fleet `scripts/replication/` and its
  SETUP-replication runbook. `create` bypasses all of it, so onboarding silently produces a
  half-wired repo.
- **The client is the right actor to provision both ends.** `gr` runs on a client that holds a
  normal per-host SSH key to **both** the primary and the backup (ADR-0009), so it can `init` and
  harden a home on either. The **primary→backup** credential is a *separate*, forced-command key
  held only by the primary — it must stay untouched. Provisioning the empty backup home from the
  client preserves least-privilege and the one-way trust direction (AC; home-fleet ADR-0002).
- **`create` already half-lives in the right place.** `io/server.rs` has `init_bare`, `set_head`,
  `home_exists`; the decision logic belongs in the functional core (ADR-0002), the SSH side
  effects in the io shell.

## Decision
When **`[backup]` is configured**, `gr create` provisions the **full fleet topology** in one
command, extending (not replacing) the ADR-0013 flow:

1. **Primary home** — `git init --bare` + `HEAD` + wire `data`/`data-lan` + push *(unchanged from
   ADR-0013)*.
2. **Primary hook** — install the `post-receive` replication hook into the new home, so every
   future push mirrors to the backup immediately (not only on the catch-up sweep).
3. **Backup home** — over the `[backup]` alias, `git init --bare` + set `HEAD` + harden
   (`receive.denyNonFastForwards`, `receive.denyDeletes`, install the `pre-receive` guard), so the
   hook/sweep has a **hardened, receive-only** target.

Properties, inherited from the existing safety posture:

- **Never-clobber / idempotent** (as ADR-0013 `create` already refuses an existing *primary*
  home): if the **backup** home already exists and is in sync, proceed; if it exists and
  **diverges**, refuse and report — never force or overwrite (SI).
- **Fail-loud, no half-acting** (ADR-0012 §5): if `[backup]` is set but unreachable, or the hook
  install / hardening fails, `create` reports the partial state and the exact remaining step
  rather than claiming redundancy it didn't achieve.
- **`gr` provisions, it does not replicate.** `create` only *creates and hardens the empty backup
  home*; it never pushes repo content to the backup. Content still flows primary→backup via the
  controller's hook/sweep under its own forced-command key. The trust direction is unchanged (AC,
  SC).
- **Hooks are vendored assets** embedded in `gr` (the `post-receive` and `pre-receive` bodies),
  written verbatim — so `gr` is self-contained and does not depend on a checkout of the companion
  project. The companion home-fleet `scripts/replication/` remains the **source of truth**; the
  vendored copies carry a version/provenance marker to make drift detectable.
- **Audited** (ADR-0004 AU): the hook install and backup-home provisioning are recorded like every
  other mutating action.
- **No `[backup]` configured** ⇒ behavior is exactly as today (primary only), but `create` warns
  that the repo is **not redundant** and points at `[backup]`.

Scope: only **`create`** (`local-only → linked`) gains topology provisioning. `clone`
(`home-only → linked`) is unaffected — the home already exists, redundantly. `sync` is unaffected.

## Consequences
- **Onboarding finally equals redundancy.** A single `gr create` yields a repo that is present on
  the primary, hooked for immediate mirroring, and backed by a hardened home on the backup — the
  two-box promise met in one verb. The `home-fleet` canary (manual `FAIL → OK`) is exactly the
  toil this removes; the remaining catalog onboards without per-repo hand-wiring.
- **`gr` now does bounded server-side setup on the backup**, not just the primary — a small
  expansion of what `gr` mutates. It is narrow by construction: create empty home + set two config
  flags + install one hook; never pushes content; never touches the forced-command replication
  key.
- **Vendored hook bodies must track the companion's `scripts/replication/`** — a drift risk,
  mitigated by the provenance marker and by naming home-fleet the source of truth. (A future step
  could fetch/verify them rather than vendor.)
- **Backup-home `set_head`/hardening reuse the primary path** in `io/server.rs`, parameterized by
  alias — modest code, fully testable in the functional core (ADR-0002), behind the same SSH
  transport (ADR-0009).
- **Parked follow-up (future ADR):** surface backup **sync state** per repo in `gr status` — not
  just ADR-0015 *presence* (`ok`/`miss`) but whether the backup is *current* with the primary
  ("see the backup state for each repo"). Deferred to keep this ADR scoped to provisioning; lag
  remains the backup-host monitor's job per ADR-0015 unless/until that ADR revisits it.
