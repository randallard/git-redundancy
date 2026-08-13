# Architecture Decision Records (ADRs)

This directory records the significant decisions for **git-redundancy**, one file per
decision, with the context and consequences — so the *why* survives, not just the *what*.

## What this is (and the "DAC?" question)

The recognized practice is the **ADR — Architecture Decision Record**, introduced by
Michael Nygard (2011). The lightweight Markdown template many teams use is **MADR**
(Markdown Any Decision Records). We use a MADR-lite form below.

> Note: **"DAC" is something else** — *Discretionary Access Control*, an access-control
> model (owner decides permissions), not a decision-documentation format. The thing you
> wanted is an **ADR**.

There is no single government-mandated format for engineering decisions; ADR/MADR is the
de-facto industry standard. (For *security control* decisions specifically, NIST RMF uses
artifacts like the SSP and POA&M — heavier, system-level. ADRs are the right grain here;
where a decision touches a control we cite the 800-53 family inline.)

## Conventions

- Files: `NNNN-kebab-title.md`, zero-padded, monotonically increasing.
- **Immutable in substance:** to change a decision, write a *new* ADR that supersedes the
  old one and flip the old one's status to `Superseded by ADR-XXXX`. Don't rewrite history.
- Status values: `Proposed` · `Accepted` · `Superseded` · `Deprecated`.
- **Every ADR names its enforcer — `Verified-by:`.** What would fail if the decision
  silently stopped being true: a test name, a CI step, or an honest `none`. An ADR that
  asserts a property nothing checks is the failure mode this line exists to make visible —
  ADR-0004's CM row claimed a pinned `Cargo.lock` for ~2 months while CI re-resolved freely,
  and nothing in the log showed it. Prefer a specific, greppable name (`ci: clippy --locked`,
  `push.rs::repoint_to_behind_home_still_pushes`) over a vague area. Use
  `none — <why, and where it's tracked>` rather than leaving it blank or inventing a verifier;
  `manual, live-verified YYYY-MM-DD` is legitimate for the SSH paths that can't be hermetic.
  Partial implementation belongs here too (see ADR-0006's unbuilt `--dirty-only`).

  > **Carve-out to immutability:** `Status` and `Verified-by` are *metadata about* the
  > decision, not the decision, and both legitimately change over the ADR's life — a test
  > added later moves `Verified-by` from `none` to a name. Editing these two lines in place
  > is expected and is **not** a rewrite. Everything below the header — Context, Decision,
  > Consequences — stays immutable; supersede instead.

## Template

```markdown
# ADR-NNNN: <title>
- Status: Proposed | Accepted | Superseded by ADR-XXXX
- Date: YYYY-MM-DD
- Deciders: <names>
- Verified-by: <test name | CI step | manual, live-verified YYYY-MM-DD | none — why>

## Context
<forces at play, constraints, what makes this non-obvious>

## Decision
<what we chose, stated plainly>

## Consequences
<results, good and bad; what this commits us to>
```

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0000](0000-record-architecture-decisions.md) | Record architecture decisions (use ADRs) | Accepted |
| [0001](0001-use-rust-for-the-cli.md) | Use Rust for the CLI | Accepted |
| [0002](0002-functional-core-imperative-shell.md) | Functional core / imperative shell | Accepted |
| [0003](0003-git-backend-hybrid.md) | Git backend: hybrid (gix local read + system `git` for network) | Superseded by 0010 |
| [0004](0004-fisma-high-aligned-not-certified.md) | FISMA-High–aligned practices, not a certification claim | Accepted |
| [0005](0005-fips-crypto-path-a-enforce-approved-algorithms.md) | FIPS crypto: enforce approved algorithms now (Path A) | Accepted |
| [0006](0006-command-scope-current-and-all-branches.md) | Command scope: current-branch and all-branches views; never auto-commit | Accepted |
| [0007](0007-future-gui-tauri-keep-rust-core.md) | Future GUI via Tauri, keep the Rust core | Proposed |
| [0008](0008-os-omarchy-on-both-ends.md) | Target OS is Omarchy (Arch-based) on both ends | Accepted |
| [0009](0009-ssh-transport-aliases-mdns-hostkey-pinned.md) | SSH transport via host aliases (mDNS, host-key pinned, FIPS enforced here) | Accepted |
| [0010](0010-system-git-for-local-reads.md) | System `git` for local reads too (supersedes 0003) | Accepted |
| [0011](0011-ci-fast-gates-plus-kani-every-push.md) | CI: fast gates always + Kani on every push (cached, separate job) | Accepted |
| [0012](0012-home-inventory-server-side-bare-repos.md) | Home inventory: discover the bare home repos on tenx, not just local working copies | Accepted |
| [0013](0013-lifecycle-commands-create-clone-sync.md) | Lifecycle commands: `create` / `clone` / `sync` for the local↔home gap | Accepted |
| [0014](0014-status-ux-lifecycle-and-repo-detail.md) | Status UX: lifecycle in the fleet view, an "others" indicator, and a repo detail view | Accepted |
| [0015](0015-backup-server-presence-column.md) | A `[backup]` server and a `Bkp` presence column in `gr status` | Accepted |
| [0016](0016-create-provisions-full-fleet-topology.md) | `gr create` provisions the full fleet topology (replication hook + hardened backup home) | Accepted |
| [0017](0017-onboard-guided-walk-and-ignore-list.md) | `gr onboard` — a guided per-repo walk, with a config `ignore` list | Accepted |
| [0018](0018-repoint-backup-only-homes-into-current-topology.md) | `repoint` — bringing a backup-only home into the current primary→backup topology | Accepted |
| [0019](0019-freshness-before-classification-status-push-fetch.md) | Freshness before classification — `status`/`push` fetch before computing ahead/behind | Accepted |
| [0020](0020-onboard-attach-existing-primary-home.md) | `onboard` gains `attach` — a working copy whose primary home already exists | Accepted |
| [0021](0021-collapse-same-server-transport-aliases-in-status.md) | Collapse same-server transport aliases into one status column | Accepted |
| [0022](0022-default-command-interactive-stage-and-commit-review.md) | Bare `gr` offers an interactive stage-and-commit review for dirty repos | Accepted |
| [0023](0023-coverage-gate-tiered-by-testability.md) | Coverage gate tiered by testability, not one blended workspace number | Accepted |
