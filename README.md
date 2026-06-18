# git-redundancy

[![CI](https://github.com/randallard/git-redundancy/actions/workflows/ci.yml/badge.svg)](https://github.com/randallard/git-redundancy/actions/workflows/ci.yml)

A small, fast CLI (`gr`) for keeping a fleet of local working repos backed up to their bare
"home" remotes — see the state of every repo at a glance, and push the *easy, committed*
work home in one command. Built for the end-of-day question: **"is all my work safely
backed up, and what still needs attention?"**

It is deliberately conservative: it **never auto-commits, never force-pushes, and never
touches a diverged branch**. It backs up what's safe and tells you loudly about the rest.

> Status: early (`0.0.0`), but the core works and is well-tested. `gr status` and `gr push`
> are implemented; see [Status](#status).

## Why

If you keep bare git repos as personal backup remotes (e.g. on a home server reachable over
LAN or Tailscale), you still need a quick, safe way to (a) check which working copies are
ahead/behind/dirty, and (b) push the committed work up — across *all* your repos at once,
without clobbering anything. That's `gr`.

## Highlights

- **Status table** — per repo: branch, working-tree state (staged / unstaged / untracked /
  conflicts), and per-remote ahead/behind, plus merge difficulty (`new` / `ok` / `↑n` /
  `↓n` / `diverged` / `CONFLICT`, detected with `git merge-tree`).
- **Safe push** — only *fast-forward* or *new-branch* pushes (easy + committed); diverged or
  behind branches are **skipped and reported**, never forced. Dirty trees are surfaced but
  never block pushing already-committed commits.
- **Transport failover** — treats configured remotes as interchangeable paths to the same
  server (e.g. LAN first, Tailscale fallback) and pushes once, via the first that works.
- **FIPS-enforced SSH** (optional, recommended) — pin the transport to FIPS-approved
  algorithms, fail-closed, via SSH host aliases. See [docs/SETUP.md](docs/SETUP.md).
- **Append-only audit log** — every push action is recorded with a UTC timestamp.
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

# Remotes to show as columns / push to, in order.
default_remotes = ["data-lan", "data"]

# Transport failover: try these in order until one succeeds (same server, two paths).
[transport]
auto = true
order = ["data-lan", "data"]

# Audit log location (on by default).
[audit]
enabled = true
# log = "~/.local/state/git-redundancy/audit.log"
```

An empty/missing config means "nothing to do" — `gr` says so rather than scanning.

To reach a home server over a FIPS-enforced SSH transport, wire up the host aliases per
**[docs/SETUP.md](docs/SETUP.md)** (`data-lan` → `tenx-lan`, `data` → `tenx-ts`).

## Usage

### `gr status`

```console
$ gr status
╭──────────────┬───────────────────┬───┬───┬───┬────┬──────────┬──────╮
│ Repo         │ Branch            │ S │ U │ ? │ Cf │ data-lan │ data │
├──────────────┼───────────────────┼───┼───┼───┼────┼──────────┼──────┤
│ api-server   │ * release-1.9     │ · │ · │ · │ ·  │ ↑2       │ ↑2   │
│ infra-notes  │ * main            │ · │ · │ · │ ·  │ ↑1       │ ↑1   │
│ local-notes  │ * master          │ · │ 3 │ 4 │ ·  │ ok       │ ok   │
│ web-frontend │ * web-release-1.9 │ · │ · │ · │ ·  │ ↑2       │ ↑2   │
╰──────────────┴───────────────────┴───┴───┴───┴────┴──────────┴──────╯
```

Columns: **S/U/?/Cf** = staged / unstaged / untracked / conflicts (`·` = none);
per-remote = `↑ahead` / `↓behind`, or `new` / `ok` / `diverged` / `CONFLICT`. In a terminal
the cells are colorized (green ahead, yellow behind/diverged, red conflicts, dim clean);
disable with `--no-color` (also auto-off when piped or `NO_COLOR` is set).
Flags: `-a`/`--all-branches` (one row per local branch), `--remote <name>` (single column).

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

## How it works

A three-crate Cargo workspace following a functional-core / imperative-shell split:

- `git-redundancy-core` — pure, no-IO logic (state types, ahead/behind & "easy push"
  classification, porcelain parsing). Property-tested and Kani-verified.
- `git-redundancy-io` — config, repo discovery, system-`git` invocation, audit logging.
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

Implemented and tested: `gr status`, `gr push` (with failover, audit logging, dry-run),
hermetic integration tests, property tests, and a Kani-verified safety invariant; CI runs
the gates + proofs on every push. Not yet: `--json` output, coverage gate.
A GUI is a possible later phase (Tauri, reusing the Rust core).

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
