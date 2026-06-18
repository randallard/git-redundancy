# 2026-06-18 — Home inventory (ADR-0012), after a course-correct

**Documents:** commits `66c18c4` ("feature add plans" — ADRs 0012–0014 + PROGRESS) and
`0c52f6c` ("feature progress" — the ADR-0012 implementation). Follows the
[CI/SSH entry](2026-06-17-ci-green-and-ssh-transport-wired.md) (`15c4230`/`d532f24`).
**Status:** `gr` now sees both sides — local working copies *and* the bare "home" repos on
tenx — via `gr homes`, verified live. The lifecycle/status UX (0014) and the
`create`/`clone`/`sync` verbs (0013) are designed but not yet built.

## How the day actually started (the course-correct, honestly)

The ask was "an easy way to create a bare repo for a project and add the remote," referencing
the git setup described in `infra-notes/USCourts_setup`. I took that too literally and built
the wrong thing first: a standalone **`gitserver` bash CLI** dropped in `USCourts_setup`, a
**single auto-selecting `tenx` SSH alias** (a `Match exec` TCP probe that picked LAN-vs-
Tailscale on its own), and I **relinked `USCourts_setup`** onto that one remote.

That cut directly against this project's architecture, which I hadn't re-read yet:

- **`git-redundancy` already is the CLI** — a Rust workspace (`gr`), not a place to bolt a
  shell script onto a sibling repo. CLIs are their own projects; `USCourts_setup` only
  *references* them.
- **ADR-0009 deliberately keeps two aliases** (`tenx-lan`/`tenx-ts`) and does failover at the
  **application layer** in `gr push` — so the chosen transport is visible, audited, and
  FIPS-verifiable. My single-alias probe hid that choice in SSH and skipped the FIPS algorithm
  block. Worse, "push to both" buys nothing here: both transports point at the *same* bare
  repo, so the only real need is "whichever path is up" — which `gr` already solves.

Reverted all three: removed the `tenx` alias, restored `USCourts_setup` to `data`/`data-lan`
per ADR-0009 (confirmed both resolve to the same `main`), deleted the bash script + symlink.
Lesson logged: **read the project's ADRs before adding capability that looks adjacent to it.**

## Design: three ADRs for the next increment (`66c18c4`)

With the real shape clear, we split the "create/clone/sync + see it all" work into three
decisions rather than one:

- **[ADR-0012](../adr/0012-home-inventory-server-side-bare-repos.md)** — *home inventory.* A
  repo becomes *a name with up to two presences* (local / home), giving a lifecycle:
  `local-only` / `home-only` / `linked`. Identity is the **home name**, derived from the
  `data` remote URL (so `USCourts_setup`↔`omarchy-setup` resolves with no hand-kept mapping);
  the two transports dedupe to one home. Adds a `[server]` config block; reads **degrade**
  when tenx is unreachable; cloud `origin` is out of scope.
- **[ADR-0013](../adr/0013-lifecycle-commands-create-clone-sync.md)** — `create`/`clone`/`sync`,
  all audited and server-required. `sync` is easy-only (easy-push ahead, **ff-pull behind on a
  clean tree**, diverged/CONFLICT reported) — a narrow, principled extension of ADR-0006 that
  admits the first working-tree write.
- **[ADR-0014](../adr/0014-status-ux-lifecycle-and-repo-detail.md)** — status UX: a lifecycle
  cell, `home-only` rows, a `+N⚠` "others need attention" indicator, and a positional
  `gr status <repo>` detail that previews what `sync` would do.

## Implementation: ADR-0012 (`0c52f6c`)

Functional-core / imperative-shell, per ADR-0002:

- **`core::presence`** (pure) — `home_name_from_url`, `Lifecycle`, and `join_presences`
  (keyed/deduped by home name). Unit tests + two proptests, incl. `join_lifecycle_invariants`
  (over arbitrary local/home mixes: output sorted+unique, every input represented, each
  entry's lifecycle exactly determined by which sides exist).
- **`io::inventory`** (shell) — `survey(cfg)` discovers locals, derives each home name from its
  transport remote, lists server homes over `ssh <alias> 'ls -d <root>/*.git'` (ADR-0009 alias
  failover, `ConnectTimeout=5`, `BatchMode`), and joins. Unreachable/unconfigured ⇒ local-only
  view with `reachable = false`.
- **`[server]` config** (`root`, optional `aliases`) + `server_enabled()`; `git::remote_url`.
- **`gr homes`** (+`--offline`) surfaces it — a provisional foundation view that ADR-0014 will
  fold into `gr status`.

## Verification

- **Live against tenx:** `omarchy-setup → linked → local: USCourts_setup` (identity mapping
  works), uncloned `myproject → home-only`, `cmecf_* → linked`, the rest `local-only`.
  `--offline` drops the network and the home-only row.
- **Gates:** `fmt` + `clippy -D warnings` clean; **39 tests** (core 18 · io 11 · cli 10); the
  3 Kani integer proofs untouched (the new join logic is String/collection-shaped, so it's
  proptest-verified — same boundary as `rfc3339_utc` and the porcelain parser).
- **Coverage:** installed `cargo-llvm-cov` (the previously-open CI gate, now available
  locally). Workspace **84% line**; `core::presence` **98%**; `io::inventory` **75%** after
  adding no-network unit tests (was 60%). The remaining inventory misses are the live-`ssh`
  execution lines — verified by hand, not hermetically.

## Honest debt

- **`gr homes` is provisional** — a diagnostic surface for 0012; whether it survives as such or
  fully dissolves into `gr status` is an ADR-0014 question, left open there on purpose.
- **`--offline`** is implemented by surveying with a cleared `server.root` — correct and
  shell-contained, but a touch indirect (the clean form is a `bool` threaded into `survey`).
- **Inventory's network paths aren't hermetically tested** — same family as the rest of the io
  network code; covered live, not in CI.

## Next

1. Implement **ADR-0013** — `create` / `clone` / `sync`, reusing the inventory + push-safety
   layers; audit each action; the ff-pull-on-clean-tree path is the one to get right.
2. Then **ADR-0014** — fold inventory into `gr status` (lifecycle cell, `+N⚠`, positional
   detail), at which point `gr homes` either retires or stays as a thin alias.
