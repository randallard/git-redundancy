# 2026-08-12 — A help screen, a dropped flag, and what they exposed about the ADR log

**Documents:** commit `e42e046` — `flatten_help` on the CLI, `--locked` across the CI gates,
the `Verified-by:` addition to the ADR template, and
[ADR-0023](../adr/0023-coverage-gate-tiered-by-testability.md) (Proposed).
**Status:** ✅ committed. ADR-0023 is **Proposed, not accepted** — the coverage job still runs
the old single 58% floor; the CI edit lands when the decision does.

## The itch

It started as a feature request that turned out to be a documentation bug: *"I really want an
easier way to just push all branches that are easy to `data`."* That's `gr push -a`. It has
existed since ADR-0006, is implemented on `push`, `status`, `sync` and `create`, and is tested.

The problem was that `gr --help` listed eight subcommands and **zero options**. Every flag was
one `gr <cmd> --help` away, which is one step further than anybody looks. A shipped, tested,
ADR-backed feature read as missing. Worth naming as its own failure mode: *accepted, built,
and invisible* is not meaningfully different from unbuilt.

Fix: `flatten_help = true` on the `Cli` derive, plus an `after_help` stating the bare-`gr`
default and the two standing promises (never auto-commit, never force a diverged branch).
Nine lines, no behavior change, no new flags, no default changed — `push` still defaults to
the current branch, which is what Ryan wanted. It's derive-driven, so future flags appear
without anyone maintaining a cheat-sheet.

## The dropped flag

Then I installed it with `cargo install --path crates/cli` — **without `--locked`, contrary to
this repo's own `README.md:64`.** Cargo happily resolved clap 4.6.5 over the pinned 4.6.1.

Chasing "can we disallow install without `--locked`" turned up the real finding. There is no
cargo-native way ([rust-lang/cargo#8207](https://github.com/rust-lang/cargo/issues/8207), open
since 2020; cargo aliases can't shadow built-in commands either). But looking for where to
enforce it surfaced this: **only `cargo vet` used `--locked` in CI.** `clippy`, `test`, and
`llvm-cov` all re-resolved freely. A `Cargo.lock` that no longer satisfied the manifest would
have passed every gate green.

ADR-0004's CM row has claimed "pinned `Cargo.lock`" since 2026-06-17. Nothing checked it, for
roughly two months, and nothing in the log would ever have said so. It was found by accident —
by me making the mistake the docs already told me not to make.

`--locked` now on `clippy`, `test`, `llvm-cov`. `cargo fmt` is excluded on purpose: it rejects
the flag and resolves nothing. Kani and `cargo-cyclonedx` run through wrapper actions and were
left alone rather than guessed at — **still open.**

## The audit that fell out of it

If one ADR asserted something unenforced, how many others? Auditing the log against the code
found four instances, and usefully they are four *different* failure modes:

| Mode | Instance |
|---|---|
| ADR promises what was never built | ADR-0006's `--dirty-only` — zero hits in `crates/`, Status `Accepted` since 2026-06-17 |
| ADR asserts what nothing enforces | ADR-0004's pinned lockfile (above) |
| CI enforces what no ADR decided | the coverage floor — `grep '70%\|fail-under\|coverage'` across 0004 and 0011 returns **nothing** |
| Code moved before its ADR | 0003 (gix) deviated in code, 0010 legitimized it after |

Plus a latent one: `docs/adr/README.md` hand-maintains an index table duplicating every ADR's
Status — two sources of truth for one field.

Mode 4 is arguably not a bug. ADR-0010 is a *good* ADR precisely because it recorded a
decision reality had already made. The goal is that the record catches up, not that code waits.

**What landed:** `Verified-by:` in the MADR-lite template — every ADR names what would fail if
its decision silently stopped being true (a test name, a CI step, `manual, live-verified
<date>`, or an honest `none — why`). ADR-0004 would have had to write `none` for the lockfile
claim, which is a visible lie somebody catches.

This required a carve-out to ADR immutability, stated explicitly in the README: `Status` and
`Verified-by` are metadata *about* the decision, not the decision, and both legitimately change
over an ADR's life — a test added later moves `Verified-by` from `none` to a name. Editing
those two header lines in place is expected and is not a rewrite. Context / Decision /
Consequences stay immutable; supersede instead.

## The coverage floor was never a quality problem

Writing [ADR-0023](../adr/0023-coverage-gate-tiered-by-testability.md) meant finally measuring
instead of repeating the CI comment. The comment and PROGRESS both said ~60% / 59.95%. Actual,
2026-08-05:

| Scope | Lines |
|---|---|
| Pure core (`-p git-redundancy-core`) | **96.37%** |
| Workspace minus the network shell | **84.42%** |
| Whole workspace (today's gate) | **65.51%** |

The blended number was averaging a nearly-fully-covered pure core against SSH orchestration
that cannot be hermetically tested, and describing neither. The "COVERAGE DEBT — we lowered the
bar and must raise it" framing, carried since 2026-06-22 with no expiry, had it backwards: the
**testable surface is already at 84.42%, above the *original* 70% bar.** It was a denominator
problem wearing a quality problem's clothes, and it hid the fact that the code we can test got
*better* while the headline sagged.

ADR-0023 proposes three tiers — core ≥95%, testable surface ≥80% (network shell excluded by an
exhaustive, closed three-file list), whole workspace reported but ungated. Net bar goes *up*
versus both the 58% in force and the 70% before it. The exclusion list is closed by design:
adding a file requires superseding the ADR, which is what stops it becoming a place to hide
untested code.

Left `Proposed`. The substance is a recommendation and Ryan is the Decider — and it would be a
poor first outing for a convention about honest records to have the model quietly accept its own
proposal.

## Postscript: the gates weren't running

Before starting the follow-up work, Ryan asked whether any of this had already been done on a
remote. Nothing had — no branches on any of the four remotes (`origin`, `data`, `data-lan`, and
`waed-7561`, another working copy I hadn't known about), no PRs, no issues, no stashes, no
worktrees. Local was strictly ahead of everything.

But the check turned up something better. **CI last ran on 2026-06-22.** `origin/main` sits at
`81c8af5`, five commits behind local; `230de1a`, `ae41e8f`, `473cf98`, `809a456` and `e42e046`
have never been through it. The home remotes are current — `gr` does its job — but CI lives on
GitHub, and `origin` is a separate manual push that hadn't happened in seven weeks.

Which means the `--locked` fix above is, as of writing, still unexercised by the thing it fixes.
And the whole exercise nearly repeated itself: I was about to layer three new coverage gates
onto a pipeline whose real state was unknown, tuned against a 58% floor set five commits and one
whole `review.rs` ago. The two runs immediately before that floor was lowered were failures.

Same disease as everything else in this entry — a control that exists in the record and not in
reality — which is a good argument that `Verified-by:` should mean *"and it ran"*, not merely
*"a job exists that would run it."*

Held ADR-0023's CI edit until `origin` gets pushed and reports.

**It reported: red.** Run `31659943282` — kani ✅, coverage ✅, supply chain ✅, fast gates ❌ on
exactly one step, `cargo-deny`. **RUSTSEC-2026-0190**: unsoundness in
`anyhow::Error::downcast_mut()` for `< 1.0.103`, published **2026-06-25 — three days after the
last green run.** Seven weeks red and silent.

Two things worth keeping from that:

*The failure vindicated the sequencing.* Pushing first meant the red came from a dependency
advisory that had been latent since June, not from the three coverage gates I'd have added on
top. Had I landed ADR-0023 first, the obvious suspect would have been the new coverage tiers, and
I'd have gone looking in the wrong place.

*Real exposure was nil, and the gate was still right.* We call `.context()` eight times and
`downcast_mut` zero — the advisory needs both. A gate tripping on a path we don't exercise is
the gate doing its job; the fix is to clear it, not to exempt it.

`cargo update -p anyhow` → 1.0.104. Which promptly cascaded, in the direction the `--locked` work
predicts: `cargo vet --locked` failed with `anyhow:1.0.104 missing ["safe-to-deploy"]`, because
exemptions are version-pinned. `add-exemption` + `prune` collapsed that to a one-line diff.
The lesson generalizes — **a lockfile bump and its `supply-chain/config.toml` entry are one
atomic change**, and `--locked` is what turns that from a thing you remember into a thing CI
insists on.

Neither `cargo-vet` nor `cargo-cyclonedx` was installed locally. Installed both rather than guess
— which is how the cyclonedx `--locked` question got a real answer (there is no such flag)
instead of an assumption.

**One hardening landed while waiting:** `timeout-minutes` on all four jobs (gates 10, coverage
15, supply-chain 10, kani 20). Prompted by Ryan asking whether Kani could cycle forever at
origin. It can't — 35s observed warm, and the three harnesses are loop-free integer decision
logic, which is exactly why no `#[kani::unwind]` bound exists and why `rfc3339_utc` was
deliberately excluded as a target. But no job had *any* ceiling, so GitHub's 6-hour default
applied. Nothing can hang today; the ceiling is for the day a harness gains a loop.

## Next

- **Decide ADR-0023.** On accept: land the three-run coverage job, retire the per-run
  `::warning::` and the STOPGAP framing.
- **Backfill `Verified-by:` across ADRs 0000–0022.** This is the actual audit — filling it in is
  what surfaces the next ADR-0004-shaped hole. Deferred, not dropped.
- **The CI linter** (every `Accepted` ADR has a non-empty `Verified-by`; README index Status
  matches each file) — deliberately held until the convention survives a few ADRs, so it doesn't
  become a gate that gets bypassed on sight.
- **Decide `--dirty-only`** — build it, or supersede that slice of ADR-0006.
- `--locked` on the Kani step, once tested. **`cargo-cyclonedx` is settled: it has no
  `--locked`/`--frozen`/`--offline` flag at all** (installed it and checked `--help`), so that
  step gets `Verified-by: none — flag unsupported upstream` rather than sitting open forever.
- Correct the stale headline figures where they still appear in the CI job comment (~60%).
- Unrelated but outstanding from the same session: three repos have **no configured home
  remote** and are not backed up at all — `ecf-data`, `get-hearings`, `spaces-game-data`.
  `gr onboard` is the walk for those.
