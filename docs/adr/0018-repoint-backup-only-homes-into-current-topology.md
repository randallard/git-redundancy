# ADR-0018: `repoint` — bringing a backup-only home into the current primary→backup topology
- Status: Accepted
- Date: 2026-06-22
- Deciders: Ryan

## Context
ADR-0017 added the `gr onboard` walk and listed `r` (repoint) as an inline option for the
**original 7** — repos set up when **tenx** was the primary, before the fleet flipped to
**acer-primary / tenx-backup** (ADR-0015/0016). ADR-0017 deliberately deferred the *mechanics* of
`r` to its own ADR. This is it.

What a repoint candidate looks like, concretely:

- The **client's** working copy has its `data` / `data-lan` remotes (ADR-0009) pointed at **tenx**.
- A home exists on **tenx** (`/data/git/<name>.git`) and **holds real history** — tenx was the
  primary the client had been pushing to.
- **No home exists on acer** (the current primary). So `gr status` classifies it as `local-only`
  against `[server]`, yet ADR-0015's `Bkp` reads `ok` — the "backup-only home" sub-state ADR-0017
  named.

The desired end state is exactly what ADR-0016 `create` produces for a fresh repo: a hooked primary
home on acer, a hardened receive-only backup home on tenx, the client pushing to the primary, and
replication flowing **acer→tenx**. But repoint is **not** `create`, and the difference is the whole
reason it needs its own decision:

- **The backup already has content.** `create` provisions an *empty* hardened backup and never
  pushes to it (ADR-0016: "`gr` provisions, it does not replicate"). Here tenx already holds the
  authoritative history. Repoint must make **acer** authoritative *without losing anything tenx
  holds* — a history-preservation problem `create` never faces.
- **Which side is the source of truth is not a given.** The client had been pushing to tenx, so the
  client is *usually* current — but it could be **behind** tenx (another machine pushed; the client
  hasn't pulled) or **diverged**. Blindly seeding acer from the client could orphan tenx-only
  history; blindly trusting tenx could discard local work. Neither is acceptable (CP/SI).
- **The backup's ff-only guard is already a safety net.** tenx's `pre-receive` +
  `receive.denyNonFastForwards`/`denyDeletes` (ADR-0016) mean any acer→tenx mirror can *only*
  fast-forward tenx — a divergence is **rejected at the backup**, not silently forced. Repoint can
  lean on this as defense in depth rather than re-implementing the check from scratch.
- **Flip order decides what a failure costs.** Until the client's remotes are rewired, the client
  is still safely pointed at the working tenx home. So rewiring must come **last**, after acer is
  provisioned, seeded, and *confirmed* mirroring to tenx — otherwise a mid-flip failure could leave
  the client pointed at a half-built primary.

## Decision
Define **`repoint`** as a distinct mutating operation — the `local-only`-but-backup-home-present →
`linked` transition — invoked as ADR-0017's onboard `r`, and (for parity with the ADR-0013
verb-per-transition pattern) addressable directly as **`gr repoint <name>`**. It reuses ADR-0016's
server-side primitives (`init_bare`, `set_head`, `install_hook`, `harden_home`, `remote_wiring`,
the `git.rs` set-url repoint) and adds the **consistency gate** and the **ordered flip** below. It
operates over **all branches** (ADR-0017 always-`-a`), is **audited** (AU, result `repointed`),
requires `[backup]` configured, and is **fail-loud, never-force, idempotent/resumable** (ADR-0012
§5, ADR-0006).

### Preconditions
- The repo is the backup-only sub-state: **home on `[backup]` present, home on `[server]` absent**,
  local working copy present and onboard-able (not detached-HEAD / commitless — ADR-0017 flags
  those instead of proceeding).
- `[backup]` is configured (otherwise there is no backup home to repoint *into*; `r` is not offered,
  per ADR-0017).

### The consistency gate (pure core, before any mutation)
Per branch, classify the **client copy against the backup home's ref** using the same ADR-0006/0012
classification `status`/`sync` already use:

- **ahead or equal** → repointable: seeding acer from the client will fast-forward (or no-op) tenx,
  which the backup's ff-only guard accepts.
- **behind** → **refuse that branch and report.** tenx holds history the client lacks; making acer
  primary from the client would strand it. The operator absorbs it first with `gr sync` (the
  existing ff-pull, ADR-0013), then re-runs repoint.
- **diverged / CONFLICT** → **refuse and report**, never force, never auto-merge (ADR-0006).

Repoint proceeds only when every in-scope branch is ahead-or-equal. The gate's job is to guarantee
the client is the **authoritative superset** of the backup *before* acer is anointed — the backup's
ff-only guard is then the second, server-side line of the same defense.

### The ordered flip (imperative shell; rewire the client last)
1. **Provision the primary on acer** — `init_bare` + `set_head` + install the `post-receive`
   replication hook (ADR-0016 steps 1–2). Acer starts empty.
2. **Seed acer from the client** — push all branches + tags from the verified-superset local copy,
   making acer authoritative. This push fires acer's new `post-receive`, beginning the acer→tenx
   mirror.
3. **Re-role the existing tenx home as backup** — `harden_home` (ff-only / no-deletes) + install the
   backup `pre-receive`, **and remove any stale `post-receive`** left over from when tenx was
   primary, so the backup never tries to mirror in the wrong direction. The home is **not**
   re-`init`ed and its content is **never deleted** — only its role and guards change.
4. **Confirm tenx is ff-consistent with acer** — verify the acer→tenx mirror succeeded (tenx
   fast-forwards from acer; because tenx already held this history, this is a no-op-or-ff). If the
   backup's ff guard **rejects** any ref, **stop and report exactly which ref diverged** — do not
   force, do not proceed to step 5. (This is the case the gate should have caught; the guard is the
   backstop.)
5. **Repoint the client's remotes** — only now rewire `data` / `data-lan` from the tenx URLs to the
   **acer** URLs via `remote_wiring` + set-url (the literal "repoint" that names the verb, and the
   same move as SETUP.md §4). The client now pushes to the primary, which mirrors to the backup.

If any step fails, repoint reports the partial state and the exact remaining step (ADR-0012 §5) and
leaves the client **still pointed at the working tenx home** (steps 1–4 touch only acer-provisioning
and tenx-hardening; neither loses data). Re-running is **idempotent**: an already-present acer home,
already-installed hook, or already-hardened backup are detected and skipped, so repoint **resumes**.

### Carried over, unchanged
- Trust direction is untouched (AC): repoint provisions acer and hardens tenx from the client's
  per-host keys; it never touches the primary→backup forced-command replication key (ADR-0016).
- `gr` still **provisions, and here also *seeds*, but never replicates**: step 2's seed push is the
  client populating the *primary* (exactly what `gr push`/`create -a` already do); ongoing
  primary→backup content still flows only via the controller's hook/sweep under its own key.
- `--dry-run` (ADR-0017) previews the plan **and the gate result** per branch — what would be
  provisioned, seeded, hardened, and rewired — without mutating or auditing.

## Consequences
- **The original 7 have a safe, one-decision path into the current topology.** repoint ends them in
  the identical state `create` gives a fresh repo — hooked primary, hardened backup, client on the
  primary — without the operator hand-rewiring remotes or hand-creating the acer home.
- **History is preserved by construction, twice.** The pre-mutation consistency gate refuses any
  branch where the client isn't ahead-or-equal of the backup, and the backup's standing ff-only
  guard rejects any non-fast-forward mirror — so repoint can only ever *advance* the backup, never
  rewrite or drop it (CP/SI). A `behind`/`diverged` repo is sent to `gr sync` first, reusing
  existing reconciliation rather than inventing new merge behavior.
- **Rewiring-last makes failure cheap.** Because the client's remotes move only after acer is built,
  seeded, and confirmed mirroring, any interruption leaves the client on the still-working tenx home
  and the operation re-runnable — no half-flipped repo.
- **Almost entirely reused mechanics.** repoint adds the consistency gate (pure `core`, already the
  `status`/`sync` classification — ADR-0002), the ordered orchestration, the stale-`post-receive`
  removal on the re-roled backup, and an explicit post-seed mirror confirmation. Everything else is
  ADR-0016's existing `io/server.rs` primitives parameterized by alias, so the new surface is small
  and testable without a network.
- **A new mutation — seeding the primary during onboarding — is named explicitly**, but it is just
  the client pushing committed branches to a home it owns (the `gr push`/`create -a` mutation class,
  ADR-0006/0013), gated by the superset check. It does not expand what `gr` writes to the backup.
- **repoint joins create/clone/sync as a first-class lifecycle verb** (ADR-0013), with `gr onboard`
  exposing it as `r` exactly as it exposes `create` as `y` — keeping the walk able to resolve
  *every* un-redundant kind, and the verb addressable on its own for the operator who already knows
  which repo needs it.
