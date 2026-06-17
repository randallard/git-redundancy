# git-redundancy — progress & design

> `git-redundancy` (binary `gr`, also installable as the git subcommand `git redundancy`)
> — a multi-repo status + push CLI for keeping local working copies backed up to their bare
> "home" remotes (`/data/git/*.git` on `tenx-rltec`). Repo:
> `git@github.com:randallard/git-redundancy.git`.
> Companion to `infra-notes/GIT_REPOS_PLAN.md`, which documents the bare-repo server,
> the `data` (Tailscale) / `data-lan` (LAN) remotes, and the server-side bundle backups.

**Status:** first working increment implemented & tested. `gr status` and `gr push` work
end-to-end (with audit logging); not yet wired to the live tenx SSH aliases. Decisions are
recorded as ADRs in [`docs/adr/`](adr/README.md) — read those for the authoritative *why*;
this doc is the working overview. See [Implementation status](#implementation-status) below.

**Decisions locked (see ADRs):** Rust CLI ([0001](adr/0001-use-rust-for-the-cli.md)) ·
functional core / imperative shell ([0002](adr/0002-functional-core-imperative-shell.md)) ·
hybrid git backend, gix local-read + system `git` for all network/merge-tree
([0003](adr/0003-git-backend-hybrid.md)) · FISMA-High *aligned*, not certified
([0004](adr/0004-fisma-high-aligned-not-certified.md)) · FIPS **Path A** (enforce approved
algorithms, fail-closed) now, validated module deferred to a certified platform/container
([0005](adr/0005-fips-crypto-path-a-enforce-approved-algorithms.md)) · current- and
all-branches views, never auto-commit ([0006](adr/0006-command-scope-current-and-all-branches.md)).
GUI later via Tauri, keep the Rust core ([0007](adr/0007-future-gui-tauri-keep-rust-core.md)).
OS is Omarchy on both client and server ([0008](adr/0008-os-omarchy-on-both-ends.md)).

**Repo:** `git@github.com:randallard/git-redundancy.git` — cloned, initial commit `d6b11d7`
(docs), branch `main`. Name settled as `git-redundancy`.

## Implementation status

Workspace: `crates/{core,io,cli}` (ADR-0002), `#![forbid(unsafe_code)]` throughout.

**Built & tested:**
- `git-redundancy-core` — `WorkingTree`/`AheadBehind`, `BranchSync` classification +
  `is_easy_push`, porcelain-v2 parser, pure RFC3339 UTC formatter. Unit + `proptest`.
- `git-redundancy-io` — config (config-first), discovery, system-`git` local reads
  (branch/status/remotes/ahead-behind/`merge-tree`), `git push`, append-only audit log.
- `gr status` — table with per-remote `↑/↓`/`new`/`diverged`/`CONFLICT`, `--remote`,
  `-a/--all-branches`.
- `gr push` — easy-only (ff/new), never force, never auto-commit, diverged/behind skipped,
  dirty surfaced; transport failover (LAN→Tailscale); `--remote`/`--only`/`--dry-run`/
  `--tags`; audit-logged (ADR-0004 AU).
- Gates green: build, `clippy -D warnings`, `cargo test` (17 tests).

**Not yet:** live SSH aliases + host-key pin (ADR-0009) so push reaches tenx over the
FIPS-enforced path; integration tests (`assert_cmd`); `kani` proof job; CI supply-chain
gates (`cargo-deny`/`audit`/`vet`, SBOM); `--json` output; colorized cells.

**Known deviation:** local reads currently shell out to system `git`, not `gix` as
ADR-0003 specifies — needs a gix swap or an ADR-0003 update (tracked in the
[implementation journal](journal/2026-06-17-first-implementation.md)).

---

## 1. Goal

A small, fast, **memory-safe and testable** Rust CLI to manage a fleet of local git
repos:

1. **`status`** — one nice table of every local repo: its remotes, ahead/behind counts
   per remote, working-tree state (staged / unstaged / untracked), and any **merge
   difficulty** that would block a clean update.
2. **`push`** — push everything that's *easy* (fast-forwardable) **and committed** to
   either *all* configured remotes or *one* named remote. **Never auto-commits, never
   force-pushes, never touches a diverged branch.**

Non-goals (for the CLI phase): committing, merging/rebasing, conflict resolution, GUI.

---

## 2. Why Rust (the language debate, recorded)

Rust is the choice for the CLI. The reasoning, in tiers — because "provable" means
different things at different cost:

- **Provable memory safety (free):** safe Rust + `#![forbid(unsafe_code)]` rules out
  use-after-free / data races by construction. No other candidate (Go, Python, TS)
  offers this.
- **Illegal states unrepresentable (cheap):** newtypes, enums, typestate, exhaustive
  `match`. Most defects in a tool like this are forgotten states; the compiler catches
  them.
- **Property testing (`proptest`):** exhaustive-ish coverage of the pure logic
  (ahead/behind math, porcelain parsing, "is this push easy?").
- **Formal proof (`kani`, bounded model checker):** prove specific pure functions never
  panic and hold invariants for all inputs up to a bound. Only feasible if the pure
  logic is isolated from IO — hence the architecture below.

### Functional core / imperative shell
The single most important design decision for "provable + testable":

- **`git-redundancy-core` (pure, no IO):** state types, ahead/behind & "easy push"
  classification, table model. Target of `proptest` + `kani`. `#![forbid(unsafe_code)]`.
- **`git-redundancy-io` (imperative shell):** git invocation, network, filesystem, config
  loading. Covered by integration tests, not formal proof.
- **`git-redundancy-cli` (binary):** arg parsing (`clap`), rendering, wiring.

This keeps the verifiable surface small and real, instead of trying (and failing) to
prove an IO-heavy program end to end.

---

## 3. Security posture — "FISMA High–aligned" (with the honest caveat)

**Honest caveat:** FISMA categorizes *systems* (FIPS 199) and applies the NIST 800-53
**High baseline** to an authorized boundary with an ATO + continuous monitoring. A local
CLI that pushes your own repos over your own LAN/Tailscale is not such a boundary. So we
**do not claim the binary "is FISMA High"** — that's an organizational status, not a code
property. What we *do* is adopt the High-baseline *engineering practices*:

| 800-53 family | What we do in git-redundancy |
|---|---|
| **SI** (integrity) | `#![forbid(unsafe_code)]`; input validation; `cargo-audit` in CI (flaw remediation) |
| **CM** (config mgmt) | pinned `Cargo.lock`; `cargo-deny` (license + source allowlist); SBOM; reproducible build |
| **SR** (supply chain) | `cargo-vet`; minimal dependency set; optionally vendored deps |
| **AU** (audit) | structured, timestamped audit log of every push action (what/where/result) |
| **AC** (access) | least privilege: only configured repos; never auto-commit; explicit remotes only |
| **SC-13** (FIPS crypto) | **Path A** ([ADR-0005](adr/0005-fips-crypto-path-a-enforce-approved-algorithms.md)): enforce FIPS-approved SSH algorithms + fail-closed now; validated module deferred to a certified platform/container. Arch cannot ship a validated module (rolling release vs CMVP freeze). |

**Backend decision ([ADR-0003](adr/0003-git-backend-hybrid.md)):** **hybrid split by the
network boundary** — `gix` (pure Rust) for **local reads only** (it never touches the
network), and **system `git`** for everything that crosses the wire (`fetch`, `push`) plus
`git merge-tree` conflict detection. This keeps the bulk of the code memory-safe with no C
in the trust base, and funnels **all transport crypto through one chokepoint** (OS
OpenSSH), which is exactly what Path A enforces and audits.

No telemetry. No network access except the explicit, user-invoked push.

---

## 4. Command surface

### `gr status` (default command)
Renders a table over the configured repos (discovered within the configured roots; see §5).
Columns:

| Column | Meaning | Source |
|---|---|---|
| Repo | dir name | discovery |
| Branch | current branch (or detached) | `HEAD` |
| Staged / Unstaged / Untracked | counts (or ✓/•) of index vs worktree vs untracked | `git status --porcelain=v2 -z` |
| Per-remote `↑ahead / ↓behind` | commits local-vs-remote-tracking, per configured remote | rev-list left-right |
| Merge | `ff` (clean fast-forward) · `diverged` · **`CONFLICT`** · `new` (no remote branch) | `git merge-tree --write-tree` (git ≥ 2.38) |

- **Merge difficulty** is detected *without touching the working tree* via
  `git merge-tree --write-tree base local remote`, which reports conflicts directly.
- Color: clean = dim, ahead-only = green, behind/diverged = yellow, conflict/dirty = red.
- **Scope:** default = current branch per repo; `-a` / `--all-branches` = one row per
  local branch.
- Flags: `--remote <name>` (limit columns), `--dirty-only`, `--json` (machine output),
  `--no-color`.

### `gr push`
Push committed work that is **easy** (fast-forwardable) only.

- **"Easy" =** target remote branch is absent (new) **or** an ancestor of local
  (fast-forward). If `behind > 0` (diverged) → **skipped + reported**, never forced.
- **Committed only:** uncommitted/unstaged/untracked changes are **never** committed and
  **never** block the push of already-committed commits — but they are **loudly warned**
  so a dirty repo never reads as "fully backed up."
- **Scope:** default = current branch; `-a` / `--all-branches` = all local branches.
- Target: `gr push` → all configured remotes; `gr push --remote data-lan` → one remote.
  `--only <repo>...` to limit repos. `--dry-run` to preview. `--tags` to include tags.
- Auto-transport (optional, config-driven): prefer `data-lan`, fall back to `data`
  (Tailscale) when the LAN host is unreachable — so the same command works home/office.
- Prints a per-repo result summary: `pushed N` · `up-to-date` · `SKIPPED (diverged)` ·
  `DIRTY (committed pushed, M files left uncommitted)`.

---

## 5. Config

TOML at `$XDG_CONFIG_HOME/git-redundancy/config.toml` (fallback `~/.config/...`):

**Config-first, not auto-discovered.** git-redundancy never scans the filesystem on its own
or assumes a default location — it acts **only** on what the config declares. You configure
**roots**, and repos are discovered *within those roots* (one level, each entry containing a
`.git`); you can also list explicit repo paths for anything outside a root. Empty/missing
config = nothing to do (it tells you so), never a surprise scan.

TOML at `$XDG_CONFIG_HOME/git-redundancy/config.toml` (fallback `~/.config/...`):

```toml
# Roots to discover repos *within* (each immediate child holding a .git is included).
# Explicitly configured — no implicit/global scan. Machine-specific paths live here, so
# the laptop and tenx can each point at their own working-copy dir.
roots = ["/data/Development"]

# Optional: explicit repo paths to include in addition to whatever the roots find.
repos = []

# Optional: paths/names to exclude even if found under a root.
exclude = []

# optional: named remote groups for `push` and column ordering
default_remotes = ["data-lan", "data"]

[transport]              # optional auto-failover for push (ADR-0009 aliases)
auto = true
order = ["data-lan", "data"]

[audit]
log = "~/.local/state/git-redundancy/audit.log"   # AU: append-only action log
```

Within a configured root, discovery is dynamic — a new repo dropped under a root appears
automatically — but the **roots themselves are always explicit**.

---

## 6. Testing strategy ("just Rust", good coverage)

- ✅ **Unit + `proptest`** on `git-redundancy-core`: ahead/behind classification, "easy push"
  decision, porcelain v2 parser, UTC formatter. Invariants in place (e.g. "easy ⇒ not
  behind", "parser never panics", known-instant timestamps). Plus io unit tests (config,
  audit append).
- ⬜ **`kani`** (CI job, can be slow) on the pure classifiers: prove no-panic + key
  invariants for all bounded inputs.
- ⬜ **Integration** with `assert_cmd` + `tempfile`: build real fixture repos, run the
  actual binary, assert table/JSON output and push behavior (incl. diverged-skip,
  dirty-warn, new-branch). *(Exercised by hand so far — not yet codified.)*
- ⬜ **Coverage** via `cargo-llvm-cov`, gate in CI (target ≥ 85% on `core`, lower bar on `io`).
- ⬜ **Supply chain / quality gates** in CI: `cargo deny`, `cargo audit`, `clippy -D
  warnings` (already clean locally), `fmt --check`, `cargo vet`.

---

## 7. Open decisions

**Resolved** (→ ADRs): git backend = hybrid ([0003](adr/0003-git-backend-hybrid.md)) ·
FIPS = Path A now ([0005](adr/0005-fips-crypto-path-a-enforce-approved-algorithms.md)) ·
branch scope = both current & `--all-branches` ([0006](adr/0006-command-scope-current-and-all-branches.md)) ·
project name = `git-redundancy` · SSH transport = `tenx-lan`/`tenx-ts` host aliases over
mDNS, host-key pinned, FIPS algorithms enforced in the alias — this also settled *where*
Path A is enforced ([0009](adr/0009-ssh-transport-aliases-mdns-hostkey-pinned.md)).

**Still open:** _(none — decision phase complete)_
- [x] ~~**Repo discovery**~~ → settled: **config-first.** Roots are always explicitly
      configured; repos are discovered *within* those roots (plus optional explicit `repos`
      and `exclude` lists). No implicit/global filesystem scan, no built-in default path —
      machine-specific roots live in each box's config. See §5.

**Implementation prerequisites** (operational, not decisions — tracked in
[`TROUBLESHOOTING.md`](TROUBLESHOOTING.md)): set the DHCP reservation for `tenx-rltec` and
record the IP · pin tenx's SSH host key into `known_hosts` · confirm tenx's `sshd` offers
the approved algorithm set · sort out tenx's suspend/idle so it stays reachable at day's end.

---

## 8. Future plans — GUI phase

Goal stated: **provable, testable, FISMA High, TypeScript.**

**Important architectural note for that phase:** TypeScript is *not* memory-safe or
"provable" in the Rust sense, and a TS rewrite would throw away the verified core. So the
recommended path is **don't rewrite the logic in TS** — instead:

- **Tauri** (Rust backend + TS/web frontend) — *recommended*. Keeps `git-redundancy-core`
  (provable, Kani-verified, FIPS-capable backend) intact and reuses it directly; TS is
  only the view layer. Best alignment with all four goals.
- *or* compile `git-redundancy-core` to **WASM** and call it from a TS app — core stays Rust
  (still provable/testable), TS handles UI only.
- TS layer gets its own assurance: `strict` + `noUncheckedIndexedAccess`, ESLint, Vitest
  / Playwright e2e, dependency audit (`npm audit` / `osv-scanner`), SBOM.
- FISMA-High *alignment* (not status) carries over: audit logging, no telemetry,
  pinned/locked deps, FIPS crypto via the Rust core, supply-chain gates.

In short: **the provable/FISMA value lives in the Rust core; the GUI phase wraps it, it
doesn't replace it.**
