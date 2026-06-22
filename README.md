# git-redundancy

[![CI](https://github.com/randallard/git-redundancy/actions/workflows/ci.yml/badge.svg)](https://github.com/randallard/git-redundancy/actions/workflows/ci.yml)

A small, fast CLI (`gr`) for keeping a fleet of local working repos backed up to their bare
"home" remotes — see the state of every repo at a glance, and push the *easy, committed*
work home in one command. Built for the end-of-day question: **"is all my work safely
backed up, and what still needs attention?"**

It is deliberately conservative: it **never auto-commits, never force-pushes, and never
touches a diverged branch**. It backs up what's safe and tells you loudly about the rest.

> Status: early (`0.0.0`), but the core works and is well-tested. `gr status`, `gr push`, the
> `create` / `clone` / `sync` lifecycle commands, and the `onboard` guided walk are
> implemented; see [Status](#status).

## Why

If you keep bare git repos as personal backup remotes (e.g. on a home server reachable over
LAN or Tailscale), you still need a quick, safe way to (a) check which working copies are
ahead/behind/dirty, and (b) push the committed work up — across *all* your repos at once,
without clobbering anything. That's `gr`.

## Highlights

- **Home-aware status table** — per repo: a **lifecycle** cell (`linked` / `local-only` /
  `home-only`), branch, working-tree state (staged / unstaged / untracked / conflicts),
  per-remote ahead/behind and merge difficulty (`new` / `ok` / `↑n` / `↓n` / `diverged` /
  `CONFLICT`), and a `+N⚠` hint for *other* branches needing attention. With a `[backup]`
  server configured, a **`Bkp`** cell shows whether each repo is mirrored to the backup too
  (`ok` / `miss` / `?`). `gr status <repo>` drills into one repo, every branch, previewing
  what `sync` would do.
- **Lifecycle commands** — `create` a bare home for a local repo, `clone` a home you don't
  have yet, or `sync` to reconcile: easy-push ahead, fast-forward-pull behind (clean tree
  only), report diverged. `-i` confirms each action.
- **Guided onboarding** — `gr onboard` walks the un-redundant repos one at a time:
  **onboard** (create + back up), **ignore** (record it as deliberately unprotected), **skip**,
  or **quit**. Each decision is saved as you go, so the walk is resumable; `--dry-run` previews
  it. Ignored repos stay visible in `gr status` as `ignored` — never silently dropped.
- **Safe by construction** — only *fast-forward* / *new-branch* pushes and clean
  fast-forward pulls; **never auto-commits, force-pushes, or auto-merges**. Diverged/behind
  are skipped and reported; dirty trees are surfaced but never block backing up committed work.
- **Transport failover** — treats configured remotes as interchangeable paths to the same
  server (e.g. LAN first, Tailscale fallback) and acts once, via the first that works.
- **FIPS-enforced SSH** (optional, recommended) — pin the transport to FIPS-approved
  algorithms, fail-closed, via SSH host aliases. See [docs/SETUP.md](docs/SETUP.md).
- **Append-only audit log** — every mutating action (push / create / clone / sync) is
  recorded with a UTC timestamp. **`--json`** output for scripting.
- **Provable core** — the safety-critical logic ("a push is only easy when not behind") is a
  pure Rust function that is **formally proven** with [Kani](https://model-checking.github.io/kani/),
  alongside property tests. `#![forbid(unsafe_code)]` throughout.
- **Config-first** — acts only on the repos you declare; no surprise filesystem scans.

## Install

Requires a Rust toolchain (via [rustup](https://rustup.rs)) and system `git` ≥ 2.38.

```bash
git clone git@github.com:randallard/git-redundancy.git
cd git-redundancy
cargo install --path crates/cli --locked
```

This builds in release mode and installs `gr` to `~/.cargo/bin`. Verify:

```bash
gr --version
```

Update after pulling changes with `cargo install --path crates/cli --locked --force`.

## Configure

`gr` is config-first: it only touches repos you list. Create
`~/.config/git-redundancy/config.toml` (`$XDG_CONFIG_HOME` honored):

```toml
# Repos to back up (explicit list)...
repos = [
  "/data/Development/api-server",
  "/data/Development/web-frontend",
]

# ...or discover them within roots (each immediate child holding a .git):
roots = ["/data/Development"]

# Repos to deliberately leave unprotected. `gr onboard`'s "ignore" appends here;
# they still appear in `gr status` as `ignored` (never silently hidden).
ignore = ["scratchpad", "vendored-thing"]

# Remotes to show as columns / push to, in order.
default_remotes = ["data-lan", "data"]

# Transport failover: try these in order until one succeeds (same server, two paths).
[transport]
auto = true
order = ["data-lan", "data"]

# The bare-repo "home" on the server. Required for the lifecycle column in `gr status`
# and for `create` / `clone` / `sync`; omit it and gr stays purely local.
[server]
root = "/data/git"               # where the bare repos live on the server
aliases = ["tenx-lan", "tenx-ts"] # SSH aliases to reach it (else derived from your remotes)

# Optional: a second "backup" home server that the primary replicates to. When set,
# `gr status` shows a `Bkp` column — is each repo's home present on the backup too?
# (`ok` / `miss` / `?`). Presence only; replication lag is the backup host's own monitor.
[backup]
root = "/data/git"
aliases = ["acer-lan", "acer-ts"] # explicit (no per-repo remote to derive them from)

# Audit log location (on by default).
[audit]
enabled = true
# log = "~/.local/state/git-redundancy/audit.log"
```

An empty/missing config means "nothing to do" — `gr` says so rather than scanning. Without a
`[server]` block, `gr status` still works but shows lifecycle as `?` (it never contacts the
server).

To reach a home server over a FIPS-enforced SSH transport, wire up the host aliases per
**[docs/SETUP.md](docs/SETUP.md)** (`data-lan` → `tenx-lan`, `data` → `tenx-ts`).

## Usage

### `gr status`

```console
$ gr status
╭──────────────┬────────────┬───────────────────┬───┬───┬───┬────┬──────────┬──────┬─────╮
│ Repo         │ Life       │ Branch            │ S │ U │ ? │ Cf │ data-lan │ data │ ⚠   │
├──────────────┼────────────┼───────────────────┼───┼───┼───┼────┼──────────┼──────┼─────┤
│ api-server   │ linked     │ * release-1.9     │ · │ · │ · │ ·  │ ok       │ ↑2   │     │
│ infra-notes  │ linked     │ * main            │ · │ · │ · │ ·  │ ok       │ ok   │ +1⚠ │
│ local-notes  │ local-only │ * master          │ · │ 3 │ 4 │ ·  │ -        │ -    │     │
│ scratch      │ home-only  │   (home)          │   │   │   │    │ -        │ -    │     │
╰──────────────┴────────────┴───────────────────┴───┴───┴───┴────┴──────────┴──────┴─────╯
```

Columns: **Life** = lifecycle (`linked` / `local-only` = needs `create` / `home-only` =
needs `clone` / `ignored` = deliberately unprotected; `?` when the server isn't
configured/reachable); **S/U/?/Cf** = staged /
unstaged / untracked / conflicts (`·` = none); per-remote = `↑ahead` / `↓behind`, or `new` /
`ok` / `diverged` / `CONFLICT`; **⚠** = `+N` other branches that need attention. In a
terminal the cells are colorized; disable with `--no-color` (also auto-off when piped or
`NO_COLOR` is set).

Flags: `-a`/`--all-branches` (one row per local branch), `--remote <name>` (single column),
`--offline` (skip the server query), `--json` (machine-readable output).

Drill into one repo (by directory **or** home name) — every branch, with the action `sync`
would take:

```console
$ gr status infra-notes
infra-notes  [linked]
╭────────┬───┬───┬───┬────┬──────────┬──────┬────────────╮
│ Branch │ S │ U │ ? │ Cf │ data-lan │ data │ sync       │
├────────┼───┼───┼───┼────┼──────────┼──────┼────────────┤
│ * main │ · │ · │ · │ ·  │ ok       │ ok   │ ok         │
│   wip  │ · │ · │ · │ ·  │ new      │ new  │ push (new) │
╰────────┴───┴───┴───┴────┴──────────┴──────┴────────────╯
```

### `gr push`

Pushes easy, committed work home with LAN→Tailscale failover:

```console
$ gr push
  api-server         release-1.9            data-lan  pushed (↑2)
  infra-notes        main                   data-lan  pushed (↑1)
  local-notes        master                 data-lan  up-to-date
  ⚠ local-notes: 3 unstaged, 4 untracked — NOT backed up (commit to include)
  web-frontend       web-release-1.9        data-lan  pushed (↑2)

3 pushed · 1 up-to-date · 0 skipped · 0 failed · 1 dirty
audit log: ~/.local/state/git-redundancy/audit.log
```

A diverged branch is skipped, never forced:

```console
  myrepo             main                   data-lan  SKIPPED: diverged + CONFLICT (↑1 ↓1; never forced)
```

Flags: `-a`/`--all-branches`, `--remote <name>` (one remote, no failover),
`--only <repo>` (repeatable), `--dry-run` (preview; changes nothing, not audited),
`--tags` (also push annotated tags reachable from pushed commits).

Exit code is non-zero only on a real push **failure** (a *skip* is success).

### `gr create` / `gr clone` / `gr sync`

Close the gap between local working copies and their bare homes (needs a `[server]` block):

- **`gr create [name]`** — create a bare home on the server for the current repo, wire
  `data`/`data-lan`, and push. Refuses if a home of that name already exists. When a `[backup]`
  is configured ([ADR-0016](docs/adr/0016-create-provisions-full-fleet-topology.md)) it also
  installs the primary's `post-receive` replication hook and creates + hardens the backup home,
  so onboarding yields a **redundant** repo (primary + backup) — mirrored on the create push.
- **`gr clone <name> [dir]`** — clone a home that exists on the server but isn't here yet.
  The target must land inside a configured root (default `<roots[0]>/<name>`); `origin` is
  dropped so it stays reserved for a cloud remote.
- **`gr sync [repos…]`** — reconcile *easy* work both ways: easy-push ahead, fast-forward
  *pull* behind (clean tree only), report diverged/conflict. `-i` confirms each action, `-a`
  covers all branches, `--dry-run` previews.

All three are audited; all require the server reachable and fail loudly rather than
half-acting. (`gr homes` is a thin alias for the fleet `status` view.)

### `gr onboard`

A guided walk down the repos that aren't redundant yet, deciding each one
([ADR-0017](docs/adr/0017-onboard-guided-walk-and-ignore-list.md)):

```console
$ gr onboard
[2/4]  squaredance
        12M · main · github.com origin · last commit 2026-05-30
        7 uncommitted, 16 untracked (won't be backed up)
  onboard (y) / ignore (n) / skip (s) / quit (q) ? n
  ignored — recorded in ~/.config/git-redundancy/config.toml

[3/4]  infra-notes
        4.0M · main · no origin · last commit 2026-06-19
  onboard (y) / ignore (n) / skip (s) / quit (q) ? y
  creating bare home /data/git/infra-notes.git via acer-lan …
  …
  created `infra-notes` (2 branch(es) pushed) — redundant (primary + backup)
```

- **`y`** onboards via `create -a` (all branches → the full redundant topology, ADR-0016).
- **`n`** appends the repo to the config `ignore` list — it stops nagging but stays visible
  in `gr status` as `ignored`.
- **`s`** leaves it for now (asked again next run); **`q`** stops the walk.
- Each `y`/`n` is committed as it happens, so the walk is **resumable** — it shrinks each run
  until every repo is redundant or deliberately ignored.
- Detached-HEAD / commitless repos are **flagged** ("can't onboard as-is") rather than erroring
  mid-create. `--dry-run` previews the whole walk without prompting or changing anything.

Onboarding needs the home server reachable (it provisions on it). A fourth choice, **`r`
(repoint)**, is offered for repos whose home is on the *backup* but not the primary; its
mechanics are [ADR-0018](docs/adr/0018-repoint-backup-only-homes-into-current-topology.md)
and not yet implemented.

## How it works

A three-crate Cargo workspace following a functional-core / imperative-shell split:

- `git-redundancy-core` — pure, no-IO logic (state types, ahead/behind & "easy push"
  classification, porcelain parsing). Property-tested and Kani-verified.
- `git-redundancy-io` — config, repo discovery, system-`git` invocation, the server-side
  home inventory (SSH), and audit logging.
- `git-redundancy` (`gr`) — the CLI.

It shells out to system `git` for both reads and the push (one consistent tool, exact git
config fidelity, minimal dependencies). Network crypto funnels through one chokepoint — the
SSH transport — which is where the optional FIPS enforcement lives.

## Docs

- [`docs/SETUP.md`](docs/SETUP.md) — wiring a machine to a home server (SSH aliases, host-key
  pinning, FIPS transport, repointing remotes).
- [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) — running the gates locally: fmt, clippy,
  tests, coverage (`cargo llvm-cov`), and the Kani proofs.
- [`docs/PROGRESS.md`](docs/PROGRESS.md) — living design overview & status.
- [`docs/adr/`](docs/adr/README.md) — Architecture Decision Records (the *why*).
- [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) — operational gotchas.
- [`docs/journal/`](docs/journal/README.md) — dated development log.

## Status

Implemented and tested: `gr status` (home-aware, with a per-repo detail view and `--json`),
`gr push`, the `create` / `clone` / `sync` lifecycle commands, and the `gr onboard` guided
walk — all with transport failover and audit logging. `create` provisions the **full fleet
topology** when a `[backup]` is set (ADR-0016): primary home + `post-receive` hook + hardened
backup home, redundant from one command. `onboard` (ADR-0017) walks the un-redundant repos
y/n/s/q with a config `ignore` list; the `r`/repoint path (ADR-0018) is designed but not yet
built. Hermetic integration tests, property tests, and a Kani-verified
safety invariant; CI runs the gates + Kani + a coverage gate + supply-chain checks
(`cargo-deny`, `cargo-vet`, SBOM) on every push. Not yet: the *mandatory* (server-side) FIPS
tier. A GUI is a possible later phase (Tauri, reusing the Rust core — `--json` is the seam).

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
