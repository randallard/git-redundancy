# ADR-0022: Bare `gr` offers an interactive stage-and-commit review for dirty repos
- Status: Accepted
- Date: 2026-07-08
- Deciders: Ryan

## Context
`gr` already surfaces "this repo has unstaged/untracked work" — the `⚠ ... — NOT backed up
(commit to include)` line in `gr push`/`gr sync`, and the `S`/`U`/`?`/`Cf` columns in
`gr status` — but it deliberately never touches uncommitted work (ADR-0006: "never
auto-commit"; `gr push` never blocks on a dirty tree, it just warns loudly). Closing that gap
has always been manual: `cd` into each flagged repo, run `git status`/`git diff`/`git add`/
`git commit` by hand, repeat for every dirty repo in the fleet — the exact end-of-day loop `gr`
exists to shrink everywhere else.

The ask: fold that loop into `gr` itself — run it, see which repos are dirty, and optionally
cycle through them one at a time, reviewing and staging each changed/untracked file, without
leaving the tool.

## Decision
1. **Lives on bare `gr` (no subcommand), not a new subcommand or a flag.** Nothing today
   scripts against bare `gr`'s output — `home-fleet`'s scripts reference `gr status`/`gr push`
   only in comments/docs, never bare `gr`, and there's no cron/systemd job invoking it.
   `gr status`, `gr homes`, and `--json` are untouched: pure, scriptable, read-only views. The
   new prompt is a tail appended only after the default (no-subcommand) status print.
2. **The prompt isn't gated on `is_terminal()`.** It always prints and reads one line from
   stdin, exactly like `gr onboard`'s existing `y`/`n`/`s`/`q` walk (`lifecycle.rs`): an
   EOF/closed stdin reads as a graceful quit. This matches the one other interactive command
   `gr` already has, rather than inventing a second interactivity model, and keeps it testable
   the same way (piped stdin in `assert_cmd` integration tests).
3. **Scope is the current branch only.** Working-tree dirtiness only exists relative to
   whatever's checked out — a non-current local branch has no working tree of its own — so
   there's no `-a`/all-branches variant to design here; it's simply every discovered,
   non-`ignore`d repo whose current branch isn't clean.
4. **Per file: show a diff, then `[y/N/e]`.** `y` stages the whole file (`git add`); `e` opens
   `$EDITOR` (default `vi`) on it via a shell, then re-shows the diff and re-prompts; anything
   else (including Enter/EOF) leaves it unstaged. Whole-file only — no hunk-level staging
   (`git add -p`'s job if that's ever wanted; simpler to build and reason about for v1).
   Conflict entries (`u` porcelain-v2 records) are reported and never touched; already-staged
   entries are reported and included as-is.
5. **One commit-message prompt per repo, after its file loop.** Empty input leaves whatever
   got staged in the index without committing (never silently discarded).
6. **No auto-continue to push/sync.** After a repo goes clean, `gr` just moves to the next
   dirty repo. Backing up the new commit(s) is a separate, deliberate `gr push`/`gr sync`
   afterward — keeps this additive to the existing safe-by-construction model instead of
   folding a push into a flow that's about staging, not transport.
7. **Still honors ADR-0006.** `gr` never composes a commit or stages a file unattended here
   either — every `add` and every `commit` happens because the operator answered a per-file
   prompt and typed an actual commit message. It's an assist loop, not automation; no
   `--no-verify`, hooks run as normal.

Implementation-wise: a new pure parser, `parse_status_entries_v2_z` (`core/src/status.rs`),
sits alongside the existing `parse_porcelain_v2_z` counter — same token walk over
`git status --porcelain=v2 -z`, classifying each path into `Untracked` / `Modified` /
`StagedOnly` / `Conflict` rather than just counting (ADR-0002: functional core, property-tested
the same way). `io/src/git.rs` gained thin wrappers (`status_entries`, `diff_unstaged`,
`diff_untracked`, `add_file`, `commit`) matching its existing style. The walk itself lives in a
new `cli::review` module, invoked only from `main`'s `None =>` arm.

## Consequences
- **The everyday gap closes without leaving the tool**, and without weakening the "never
  auto-commit" guarantee — every mutation is a direct, individual answer to a prompt.
- **`gr status`/`--json` stay exactly as they were** — still safe to script/pipe/cron, since the
  new interactive tail is scoped to the no-subcommand invocation only.
- **New surface to test:** the pure entry parser (unit + property tests in `core`, same shape as
  the existing counter's tests) and the full stage/review/commit walk end-to-end via
  `assert_cmd` piped stdin (clean-fleet no-op, quit-on-empty-input, stage+commit tracked and
  untracked files, conflict reported-not-touched, empty commit message leaves work staged,
  multi-repo walk order).
- **Deferred, not decided against:** hunk-level (`git add -p`-style) staging, and an optional
  auto-push tail once a repo goes clean — both are natural follow-ups if whole-file staging or
  the manual `gr push` afterward ever feels like friction, but neither was asked for here.
