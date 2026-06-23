# ADR-0013: Lifecycle commands — `create`, `clone`, `sync` for the local↔home gap
- Status: Accepted
- Date: 2026-06-18
- Deciders: Ryan

## Context
ADR-0012 gives `gr` a two-sided picture: every repo is a *name* with up to two presences
(local working copy, bare home on tenx) and a lifecycle state — `local-only`, `home-only`,
`linked`. Seeing the gap is half the job; the other half is **closing it** safely:

1. **local-only** → a working copy with no bare home yet needs one **created** and wired.
2. **home-only** → a bare home never cloned here needs **cloning** and wiring.
3. **linked** → branches drift (ahead / behind / diverged) and need **reconciling**.

These are the first **mutating, networked** actions beyond `push`. They must inherit the
safety posture already established — ADR-0006 (never auto-commit, never force, diverged is
skipped-and-reported) and ADR-0004 AU (every action audited) — and extend it to two new
mutation classes: **creating a server-side repo** and **fast-forwarding the local working
tree** (a pull). The non-obvious forces:

- **Pulling touches the working tree**, which `push` never does. It is only safe on a clean
  tree and only when it is a true fast-forward — anything else risks an auto-merge we have
  sworn off.
- **A cloned repo must end up discoverable.** ADR-0012 discovery is config-first: a clone
  dropped *outside* every configured `root` would be invisible to `gr` the moment it exists.
- **`origin` is reserved.** The cloud remote convention (ADR-0009 / ADR-0012) keeps
  `origin` for GitLab; a fresh `git clone` would mint an `origin` pointing at tenx and
  collide with that.

## Decision
Three subcommands, one per lifecycle transition. All are **audited** (ADR-0004 AU, reusing
the existing `Audit.record`), all **require the server reachable** and fail loudly without
half-acting (ADR-0012 §5), and none ever force-push, auto-commit, or auto-merge.

### `gr create [name]` — local-only → linked
From a local working copy with no home:
- `git init --bare <[server].root>/<name>.git` on tenx over the ADR-0009 transport
  (`name` defaults to the local directory name, overridable).
- Set the home `HEAD` to the branch being pushed, so a fresh bare repo doesn't read as
  empty (the documented `git init --bare` gotcha).
- Wire **both** remotes, `data` and `data-lan`, per ADR-0009.
- Push the current branch (or all branches with `-a`).
- **Refuses if a home of that name already exists** — that repo is not `local-only`; the
  message points to `sync`.

### `gr clone <name> [dir]` — home-only → linked
From a bare home with no local copy:
- The clone target **must resolve to a path inside a configured `root`** so it is
  auto-discovered afterward (ADR-0012). `[dir]` omitted ⇒ default `<roots[0]>/<name>`.
- If `[dir]` is given but lies **outside every configured root** (or no roots are
  configured), `gr` does **not** clone: it prints the current roots and how to add one to
  the config, and stops — that is the user's move, not an implicit config edit.
- Clones over the ADR-0009 transport, wires `data` / `data-lan`, and **removes the
  clone-created `origin`** so `origin` stays reserved for the cloud convention.

### `gr sync [repo...]` — reconcile a linked repo, easy work only
Bring local and home into agreement by doing **only what is safe and easy**, per branch:
- **ahead / new branch** → easy-push (exactly `gr push`'s rule, ADR-0006).
- **behind** → **fast-forward pull**, but **only when the working tree is clean**. A dirty
  tree blocks the pull for that branch (reported, never stashed or force-checked-out).
- **diverged / CONFLICT** → reported, never force-pushed, auto-merged, or auto-committed.
- **Default is non-interactive:** each action is **printed and audited** as it happens.
  **`-i` / `--interactive`** prompts to confirm or cancel **each affected branch**
  individually, for when you want to walk through it.
- **Scope follows the ADR-0006 `-a` model:** `gr sync` = current branch per repo;
  `gr sync -a` = the full run across all local branches; positional `repo...` / `--only`
  limit which repos. `--dry-run` previews without touching anything (and is not audited,
  consistent with `push`).

## Consequences
- The three lifecycle states from ADR-0012 each have exactly one verb to resolve them, with
  one consistent safety story: easy/clean/forward-only, everything hard surfaced not forced.
- A **new mutation class — the fast-forward pull — is admitted**, narrowly: clean tree +
  true fast-forward only. This is the one place `gr` now writes to a working tree; the
  clean-tree gate keeps it from ever resembling an auto-merge or clobber. (This deliberately
  extends ADR-0006, which had pushing in view only; ADR-0006's *spirit* — never force, never
  auto-commit, skip-and-report the hard cases — is preserved.)
- `clone` cannot silently create an invisible repo: refusing out-of-root targets keeps
  discovery config-first and makes the roots config the single source of "where repos live,"
  at the cost of one extra "fix your config, then retry" round-trip in that case.
- `origin` stays cloud-only by construction, so the redundant-home remotes (`data`/`data-lan`)
  and the cloud remote never blur together.
- All three actions land in the AU audit log alongside `push`, so creating, cloning, and
  fast-forwarding are as accountable as pushing (`create`/`clone` record with branch `-`
  and a `created`/`cloned` result).
- Functional core / imperative shell holds (ADR-0002): SSH init, clone, fetch, push and the
  worktree fast-forward are imperative-shell IO; the per-branch decision (push? ff-pull?
  report?) is the pure `core` classification already exercised by `status`/`push`.
- `gr sync` is effectively a superset of `gr push` (it adds the pull direction). `push`
  remains as the push-only, working-tree-never-touched tool for when that is all you want.
