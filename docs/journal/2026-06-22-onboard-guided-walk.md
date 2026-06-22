# 2026-06-22 — Onboard: a guided per-repo walk, with a config ignore list (ADR-0017)

**Documents:** commit `039bbba` ("feat(onboard): guided y/n/s/q walk with a config ignore
list"), which follows `66d56c9` ("docs: ADR-0017 … and ADR-0018"). The ADRs landed first
(the project's ADR-before-feat habit, as ADR-0016 did), then the implementation.
**Status:** `gr onboard` is built, tested, and green. It's the connective verb between
*seeing* the gap (ADR-0014's `+N⚠`) and *closing* it (ADR-0016's one-command redundancy) —
a walk you drive one decision at a time. The `r`/repoint branch of the walk is designed
(ADR-0018) but not yet wired.

## What landed

A new subcommand that reuses the `gr status` survey for discovery and then walks the
**local-only, non-ignored** repos, one decision each:

- **`gr onboard`** — for each candidate prints a context block (size · branch · origin ·
  last-commit, plus an uncommitted/untracked warning), then prompts
  `onboard (y) / ignore (n) / skip (s) / quit (q)`.
  - **y** → `create -a` (all branches → the full ADR-0016 topology). The create core was
    extracted from `run_create` into **`create_home`**, now shared by `gr create` (the cwd)
    and `onboard` (any repo path) — same provisioning, no duplication.
  - **n** → append the repo to a top-level **`ignore`** array in `config.toml`.
  - **s** → skip (asked again next run); **q** → quit. Every y/n commits as it happens, so
    the walk is **resumable** and shrinks each run until everything is redundant or ignored.
- **The `ignore` list** — config-first (ADR-0017), written via **`toml_edit`** (already in the
  lockfile through `toml 0.8`) so the file's comments and formatting **survive** the edit.
  Matched by home name or directory name.
- **`ignored` lifecycle** — ignored repos stay **visible** in `gr status` as `ignored`, with
  the `+N⚠` nag suppressed. A chosen non-action never becomes a silent gap.
- **`--dry-run`** — previews the whole walk (what each repo is, what would happen) without
  prompting or mutating.
- Two small `io::git` helpers — `last_commit_date` (`%cs`) and `has_commits` — feed the
  context line and the pre-flight.

## How the decisions showed up

- **ADR-0017 walk shape** — y/n/s/q exactly as specified; `r` is *offered* for the backup-only
  sub-state (home on the backup, not the primary) but routes to a "not yet implemented
  (ADR-0018)" message and leaves the repo untouched, so the walk stays honest about what it
  can do today.
- **Fail-loud, no half-acting (ADR-0012 §5)** — onboarding provisions on the home server, so a
  detached-HEAD / commitless repo is **flagged** up front ("can't onboard as-is") rather than
  erroring partway through `create`; an unreachable server refuses the whole walk.
- **Functional-core / imperative-shell (ADR-0002)** — discovery/classification is the existing
  `survey`; the new code is the shell walk-driver plus the `ignore` read/append.
- **Audit (ADR-0004 AU)** — onboarding reuses `create`'s records; the walk adds no new audit
  verb. `--dry-run` performs nothing and isn't audited.

## Verification

- **Hermetic** (no SSH): `config::append_ignore` round-trip — asserts the entry lands
  **top-level** (not absorbed into a later `[table]`), is de-duped, and the file's comment +
  other sections survive; plus a missing-file create. CLI: onboard no-server guidance,
  onboard unreachable-server "fail loud", and the `ignored` lifecycle showing in `status`.
- **Gates:** `fmt` + `clippy -D warnings` clean; **63 tests** (core 21 · io 19 · cli 23);
  the 3 Kani integer proofs untouched.

## Honest debt

- **`r`/repoint is a stub.** The walk detects the backup-only sub-state and offers `r`, but the
  mechanics (consistency gate + rewire-last flip) are ADR-0018 and unbuilt — choosing `r`
  currently just leaves the repo as-is. The original 7 still need it.
- **Coverage of the walk driver is thin in CI** — the prompt loop and the `y`→`create_home`
  path run against a live server, so they're hand-verified, not hermetic. Same inherent
  limitation as the rest of the network code (see the [lifecycle
  entry](2026-06-18-2-lifecycle-commands.md)).
- **Pre-existing `fmt` drift in `io/server.rs`** surfaced (my local rustfmt rewraps two lines
  the committed tree didn't) — left untouched to keep this changeset scoped; likely a
  rustfmt-version difference worth reconciling separately.

## Next

- Implement **ADR-0018** — `repoint` for the backup-only homes, turning the walk's `r` from a
  stub into the real flip: provision + seed the primary, re-role the existing backup home,
  confirm the mirror, then rewire the client last. That makes `onboard` able to resolve *every*
  un-redundant kind, and finally onboards the original 7.
