# 2026-08-13 — Closing the gap between the ADR log and reality

**Documents:** commits `db2ed12` (accept ADR-0023, tier the coverage gate), `4faa19c`
(`--dirty-only`), `53592a6` (`Verified-by:` backfill), `7e1106d` (deny bans enforcing
ADR-0010), `482d553` (hermetic ADR-0015 tests via a stub `ssh`), `5c37ee8` (consolidated
BACKLOG). Continues [2026-08-12](2026-08-12-help-discoverability-locked-ci-and-adr-honesty.md),
whose `Next:` list this entry works down — and supersedes, per the append-only rule.
**Status:** ✅ all six committed and pushed; CI green on each (last: run `31666395204`).

## The theme, stated once

Yesterday's entry found four ways the ADR log had drifted from the code. Today was spending
the `Verified-by:` field — and the recurring lesson is narrower and sharper than "write things
down":

**A check that has never been observed failing is not a check.** It showed up four times:

- `cargo-deny` reporting `bans ok` on a tree containing none of the banned crates — which is
  identical output to a typo'd crate name.
- Four new tests passing green on first run, which is identical to four tests asserting nothing.
- A CI workflow that *exists* in `ci.yml` — and had not run in seven weeks.
- A `Verified-by:` line that names a job, where the job never executes.

So each thing landed today was made to fail on purpose before being trusted.

## What landed

**ADR-0023 accepted; the coverage gate is now three gates** (`db2ed12`). Core ≥95% (96.37%),
testable surface ≥80% with the network shell excluded (84.30%), whole workspace reported but
ungated (65.51%). The measurement is the whole story: the "COVERAGE DEBT" carried since
2026-06-22 was a **denominator** problem, not a quality one. The testable surface was already
above the 70% bar that predated the stopgap — the blend was hiding improvement, not decay. So
the debt is resolved by raising the bar, not by deferring it a third time. The exclusion list
is three files by name, closed; extending it needs an ADR superseding 0023.

**`--dirty-only` built** (`4faa19c`) — ADR-0006 specified it on 2026-06-17 and `grep -rn
dirty_only crates/` returned nothing for eight weeks while the ADR read `Accepted`. Filtering
happens *before* `refresh_columns`, so it is also the fast path on a large fleet; home-only
rows are suppressed because they have no working tree and can never be dirty.

**`Verified-by:` backfilled across all 23 prior ADRs** (`53592a6`). The product of this was not
the 23 filled-in lines but the **nine that could not honestly name a verifier**. Two were
closed the same day; seven remain, and several *should* remain — 0008 is an environment
assumption, 0007 is a future phase, 0000 is the process ADR itself.

**ADR-0010 enforced** (`7e1106d`). "No libgit2 in the trust base" was true only by luck:
`Cargo.lock` had zero hits, but nothing would have failed if it came back. `[bans] deny` now
names `git2`, `libgit2-sys` (banned at the `-sys` crate so it cannot arrive transitively) and
`gix`. Proven by banning `serde`, which *is* in the tree, in a throwaway config: `error[banned]`
and exit 2.

**ADR-0015 covered hermetically** (`482d553`) — and this is the one with legs. The ADR's own
`Verified-by: none` said a hermetic test "needs a second fake server." It doesn't. `gr` reaches
a server only through `ssh <alias> "ls -d <root>/*.git ..."`, and `Command::new("ssh")` resolves
via `PATH`. A stub `ssh` in a tempdir prepended to `PATH` — strip the `-o` pairs, take the alias,
run the rest **locally** — turns a "home server" into an ordinary directory of `*.git` dirs.
Four tests, one per documented state.

Two details that made those tests real. First, assertions read the `--json` `backup` field, not
the rendered table: the table's untracked-count column is *also* headed `?`, so a table match
could not distinguish an unreachable backup from a file count. I only noticed by rendering all
three states and looking at them. Second, every assertion was **mutation-tested** — break the
fixture beneath it, confirm it goes red. All four did.

**The stub is the reusable asset.** Six ADRs (0012, 0013, 0016, 0017, 0018, 0020) were
live-verify-only because faking a server looked expensive. They are all newly cheap, and the
same stub is the seed of the SSH mock that would let files leave ADR-0023's exclusion list.

## The one I got wrong

Ryan asked, at the end: *"No backlog?"*

There wasn't one. The running to-do list I had been reciting after every step lived in
conversation state and in scattered journal `Next:` sections. Checking rather than answering
from memory: "three repos have no home remote" — the only open item where **data is actually at
risk** — appeared in two journal entries and **nowhere in PROGRESS**. "ADR-0017/0018 not
live-verified" existed only inside a stale *"Where we were (2026-06-22)"* block. Two of four
items would have evaporated on a cleared context.

A control that exists in the record and not in reality — in my own working notes, on the day
spent fixing exactly that everywhere else. `5c37ee8` adds a single consolidated
`## BACKLOG — next steps`, ordered so the item that risks losing work comes first, with the
top-of-file anchor pointing straight at it.

A smaller version of the same: the first draft of that section said "seven" and then listed
eight, wrongly including ADR-0005 (which carries `manual, live-verified` — a real verifier, not
a gap). Fixed by `grep`, and the regenerating grep is now written into the entry so the count
can be re-derived instead of believed.

## Docs swept

`README.md` still described the 58% stopgap; `DEVELOPMENT.md` still documented a **70%** floor
it had never been updated from — stale through two successive changes. Both now describe the
three tiers, `--locked`, the job timeouts, and the ADR-0010 bans. `README.md` gained
`--dirty-only`.

## Next

Everything actionable now lives in
[PROGRESS.md § BACKLOG](../PROGRESS.md#backlog--next-steps) — that list is the source of truth,
not this section. In priority order it opens with: **`ecf-data`, `get-hearings` and
`spaces-game-data` have no home remote and are not backed up at all** (needs Ryan's hands —
`gr onboard` is interactive and provisions on the server), then a `gr push` to bring the home
remotes current, then ADR-0020's hermetic test as the cheapest verification item now that the
stub exists.
