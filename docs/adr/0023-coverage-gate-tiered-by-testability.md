# ADR-0023: Coverage gate tiered by testability, not one blended workspace number
- Status: Accepted
- Date: 2026-08-05 (accepted 2026-08-13)
- Deciders: Ryan
- Verified-by: `ci: coverage gate` (`.github/workflows/ci.yml`) — three `cargo llvm-cov
  --fail-under-lines` runs; this ADR's numbers are reproducible with the commands in
  "Decision" below.

## Context
The coverage gate has never had a decision record. The 70% floor, its 2026-06-22 lowering to
58%, and the `::warning::` that fires on every CI run live only in a YAML comment and in
PROGRESS.md — no ADR decided any of it. That is the inverse of the ADR-0004 problem: there,
an ADR claimed a property nothing enforced; here, CI enforces a gate no ADR ever chose. Both
are the log failing to describe reality.

The 58% floor was explicitly labelled "a STOPGAP, not a new standard," to get CI honest-green
after `onboard` (ADR-0017) and `repoint` (ADR-0018) roughly doubled the SSH-orchestration
code. It has now stood ~6 weeks with no expiry and no owner. A stopgap with no review date is
just a lowered standard that nobody voted for.

Measured 2026-08-05 (`cargo llvm-cov --locked --workspace --summary-only`), the blended
headline is **65.51%** lines — not the ~60% the CI comment and PROGRESS still claim, because
`review.rs` and its tests landed since. The blend is the problem: it averages a pure core that
is nearly fully covered against a network shell that cannot be hermetically tested, and the
resulting single number describes neither.

Splitting the same run by testability:

| Scope | Lines | What it is |
|---|---|---|
| Pure core (`-p git-redundancy-core`) | **96.37%** | ADR-0002's functional core; also the Kani/proptest target |
| Workspace minus the network shell | **84.42%** | everything a hermetic test can reach |
| Whole workspace (today's gate) | **65.51%** | the above, blended with un-mockable SSH orchestration |

The network shell is `cli/src/lifecycle.rs` (24.85%), `io/src/server.rs` (49.07%), and
`io/src/git.rs` (61.97%) — SSH/subprocess orchestration whose correctness is established by
live round-trips against tenx, not by line execution in CI. Chasing a line percentage there
buys mock-shaped tests that assert the mock.

The upshot: the testable surface is **already at 84.42%**, comfortably above the *original*
70% bar. The 58% floor was never measuring a quality problem — it was measuring a
denominator problem, and it masked the fact that the code we can test got better, not worse.

## Decision
Replace the single blended floor with **three gates, tiered by what is actually testable**,
all in the existing `coverage` CI job:

1. **Pure core ≥ 95%** — `cargo llvm-cov --locked -p git-redundancy-core --fail-under-lines 95`
   (measured 96.37%). This is the ADR-0002 verifiable surface; it should stay near-total.
2. **Testable surface ≥ 80%** — the workspace with the network shell excluded, via
   `--ignore-filename-regex '(cli/src/lifecycle\.rs|io/src/server\.rs|io/src/git\.rs)'`
   (measured 84.42%). This is the real quality gate and it is **stricter than the 70% it
   replaces**.
3. **Whole workspace: reported, not gated** — printed each run with no `--fail-under` so the
   blended trend stays visible without any number being load-bearing.

The exclusion list in gate 2 is **exhaustive and closed**: those three files, named
explicitly, no globs. Adding a file to it requires a new ADR superseding this one — that is
what stops the exclusion from becoming a place to hide untested code.

Retire the per-run `::warning::` and the "COVERAGE DEBT / STOPGAP" framing. The debt as
framed — "we lowered the bar and must raise it" — is resolved: the bar on testable code is
higher than it ever was. What remains is not debt but a standing property of the design, so
it gets stated plainly in the job comment instead of shouted on every build.

## Consequences
- The gate mirrors the architecture. ADR-0002 split pure core from imperative shell; the
  coverage gate now measures them on their own terms instead of averaging them into a number
  that describes neither.
- **The bar goes up, not down** — 80% on testable code versus the 70% that predated the
  stopgap, and versus the 58% in force today.
- CI gets slower: three `llvm-cov` runs instead of one. Cache-warm this is cheap; if it
  becomes a problem, gates 1 and 3 can be derived from one instrumented run with two reports.
- The network shell is now *explicitly* unmeasured rather than quietly diluting a blend. Its
  assurance story is live verification, and it stays honest only as long as the exclusion
  list stays closed — hence the supersede-to-extend rule.
- A future SSH stub would let files move out of the exclusion list and raise gate 2. That is
  now an unblocked improvement rather than a precondition for being honest-green.
- ADR-0011 is untouched: it decided *which* jobs run and when. This decides only what the
  coverage job asserts.

## Notes
Written `Proposed` — the substance was a recommendation and Ryan is the Decider. **Accepted
2026-08-13** and the three-gate coverage job landed with it. Acceptance deliberately waited on
a green CI baseline (run `31660947110`, `59cebc0`): the first run in seven weeks failed on an
unrelated `anyhow` advisory, and landing new gates before that was cleared would have made the
tiers the obvious suspect for a failure they did not cause.
