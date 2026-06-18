# ADR-0014: Status UX — lifecycle in the fleet view, an "others" indicator, and a repo detail view
- Status: Accepted
- Date: 2026-06-18
- Deciders: Ryan

## Context
ADR-0012 lets `gr` see both presences of a repo (local working copy, bare home on tenx) and
ADR-0013 adds the verbs to reconcile them. This ADR decides how that richer picture is
**shown**, building on the `gr status` table from ADR-0006.

Two needs pull against each other:

- **A fast end-of-day glance** over the whole fleet — "is everything backed up?" — which
  wants *one row per repo* and must not drown in branch detail.
- **The full per-branch truth for one repo** — every branch's drift, including branches not
  checked out locally and homes not cloned at all.

ADR-0006 already separated current-branch (default) from all-branches (`-a`). But the new
states from ADR-0012 don't fit the old row: a repo can now be `home-only` (no local branch
to show at all), and a repo's *current* branch can be in sync while **other** branches are
not — invisible in a current-branch view, so a clean-looking row could be hiding work that
isn't backed up. The glance view needs to *hint* at that without expanding to every branch.

## Decision
Extend `gr status` along two axes — a lifecycle-aware fleet row with an "others" hint, and a
positional detail view — rather than adding new top-level verbs.

### Fleet view — `gr status` (one row per repo)
Each row gains:
- **Lifecycle** cell: `local-only` · `home-only` · `linked` (ADR-0012). `home-only` repos
  **appear as rows** even though there is no local working copy.
- The **current branch's** sync against the home (the existing per-remote `↑/↓` / `new` /
  `diverged` / `CONFLICT` columns, ADR-0006), for `linked`/`local-only` repos.
- An **"others" indicator** — a compact `+N⚠` — where **N counts the *other* branches that
  need an action**: ahead, behind, diverged/CONFLICT, local-only (never pushed), or
  home-only (present on the home, absent locally). The branch already shown on the row is
  excluded. `+0` (nothing else outstanding) renders blank, so a fully-backed-up repo is
  visibly quiet. This is what stops a healthy *current* branch from masking drift elsewhere.

### Detail view — `gr status <repo>` (one repo, all branches)
- A **positional argument** on `status`: bare `gr status` is the fleet view; `gr status
  <repo>` is the **one-repo, all-branches** detail — the expansion of that repo's `+N⚠`.
- Works whether the repo is **local or home-only** (for `home-only` it lists the home's
  branches straight from inventory; there is simply no working-tree column).
- One row per branch: branch, per-remote sync, working-tree state where applicable, and the
  **action `sync` would take** (push / ff-pull / *blocked: dirty* / *report: diverged*),
  so the detail view reads as a preview of `gr sync <repo>`.
- `<repo>` is matched by the ADR-0012 **home name** (also accepting the local directory
  name), so `gr status omarchy-setup` and `gr status USCourts_setup` resolve to the same
  repo.

### Carried over, unchanged
- `-a` / `--all-branches` still expands the **fleet** view to one row per branch (ADR-0006);
  with every branch shown, the `+N⚠` indicator is unnecessary and omitted there.
- Existing flags keep their meaning: `--remote`, `--dirty-only`, `--json`, `--no-color`,
  plus `--offline` (ADR-0012) for a fast local-only view.
- Color stays as ADR-0006/PROGRESS §4: clean = dim, ahead-only = green, behind/diverged =
  yellow, conflict/dirty = red. The `+N⚠` hint takes the worst color among the branches it
  counts.
- Server-unreachable degradation (ADR-0012 §5): home-derived cells render `?` with a
  one-line note and `status` still exits 0.

## Consequences
- One command, three zoom levels — `gr status` (fleet glance), `gr status -a` (fleet, every
  branch), `gr status <repo>` (one repo, every branch) — instead of a separate `show` verb,
  keeping the surface small and the mental model continuous.
- The `+N⚠` indicator closes the "clean current branch hid un-backed-up work" gap that
  current-branch scope otherwise leaves open, while keeping the glance to one row per repo.
- `home-only` repos are now visible in the fleet (and openable in detail) before any local
  clone exists — the discovery hook for `gr clone` (ADR-0013).
- The detail view doubles as a **dry-run preview of `gr sync`**: the per-branch "action"
  column is the same classification `sync` acts on, so what you see is what `sync` will do.
- Slightly more to compute per row (the "others" scan reads every branch even in the
  current-branch view), but it is the same `core` classification already used elsewhere and
  needs no extra network beyond the inventory `status` already fetched.
- A positional `<repo>` on `status` constrains repo identifiers from ever colliding with
  `status`'s own flags, but not with subcommand names — `status` is explicit, so a repo
  named like a subcommand (`push`, `sync`) is still addressable as `gr status push`.
- Functional core / imperative shell holds (ADR-0002): row/indicator/detail assembly and the
  per-branch action labels are pure `core` over already-fetched state; rendering is the CLI
  shell.
