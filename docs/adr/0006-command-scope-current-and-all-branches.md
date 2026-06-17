# ADR-0006: Command scope — current-branch and all-branches views; never auto-commit
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
Two needs: a *focused* end-of-day view (just the branch I'm on, per repo) and a *complete*
view (every local branch). Same applies to pushing. The repos in scope are pinned to
specific branches (`release-1.9`, `web-release-1.9`, `master`, `main`), so the common
case is current-branch, but a full backup wants all branches.

## Decision
Both views, via a consistent flag on both commands. Default = current branch; `-a` /
`--all-branches` = every local branch.

**`gr status`** — table over discovered repos:
- Default: current branch per repo. `--all-branches`: one row per local branch.
- Columns: repo, branch, staged / unstaged / untracked, per-remote `↑ahead/↓behind`,
  merge state (`ff` / `diverged` / `CONFLICT` / `new`).
- Flags: `--remote <name>`, `--dirty-only`, `--json`, `--no-color`.

**`gr push`** — push committed, *easy* work only:
- Default: current branch. `--all-branches`: all local branches.
- `--remote <name>`: one remote; otherwise all configured remotes.
- **"Easy" = fast-forward or new branch.** Diverged (`behind > 0`) → **skipped + reported**,
  never forced.
- **Never auto-commits.** Uncommitted/unstaged/untracked never block pushing already-
  committed commits, but are **loudly warned** so a dirty repo never reads as "fully backed up."
- `--only <repo>...`, `--dry-run`, `--tags`.
- Optional config-driven transport failover: prefer `data-lan`, fall back to `data`
  (Tailscale) when the LAN host is unreachable.

## Consequences
- One mental model (`-a`) across status and push.
- Safe by default: no auto-commit, no force, diverged branches skipped — matches the
  "back up the easy committed stuff" intent without surprising history changes.
- Dirty-state is surfaced, never silently included.
