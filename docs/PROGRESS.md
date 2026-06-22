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
git backend: system `git` for reads **and** network, core stays pure
([0003](adr/0003-git-backend-hybrid.md) superseded by [0010](adr/0010-system-git-for-local-reads.md)) ·
FISMA-High *aligned*, not certified
([0004](adr/0004-fisma-high-aligned-not-certified.md)) · FIPS **Path A** (enforce approved
algorithms, fail-closed) now, validated module deferred to a certified platform/container
([0005](adr/0005-fips-crypto-path-a-enforce-approved-algorithms.md)) · current- and
all-branches views, never auto-commit ([0006](adr/0006-command-scope-current-and-all-branches.md)).
GUI later via Tauri, keep the Rust core ([0007](adr/0007-future-gui-tauri-keep-rust-core.md)).
OS is Omarchy on both client and server ([0008](adr/0008-os-omarchy-on-both-ends.md)).
**Next increment designed (not yet built):** home-aware lifecycle — discover the bare home
repos on tenx ([0012](adr/0012-home-inventory-server-side-bare-repos.md)), `create`/`clone`/
`sync` to close the local↔home gap ([0013](adr/0013-lifecycle-commands-create-clone-sync.md)),
and a lifecycle-aware status with a `+N⚠` indicator and `gr status <repo>` detail
([0014](adr/0014-status-ux-lifecycle-and-repo-detail.md)).

**Repo:** `git@github.com:randallard/git-redundancy.git` — cloned, initial commit `59dcf06`
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
- **Home inventory ([ADR-0012](adr/0012-home-inventory-server-side-bare-repos.md)):** pure
  `core::presence` (home-name-from-URL identity, `LocalOnly`/`HomeOnly`/`Linked` join);
  `io::inventory` (SSH `ls` over the ADR-0009 aliases + `git ls-remote`, graceful
  degradation); `[server]` config block; `gr homes` surfaces it (`--offline` for the local
  view). Verified live against tenx — `omarchy-setup`↔`USCourts_setup` resolves linked,
  uncloned `myproject` shows home-only. *(The fleet/detail UX folds this into `gr status` in
  ADR-0014.)*
- **Lifecycle commands ([ADR-0013](adr/0013-lifecycle-commands-create-clone-sync.md)):** pure
  `core::sync` planner (`SyncAction::plan` — push/ff/blocked/report; the ff-pull-implies-clean
  invariant is proptest-checked); `io::server` (SSH `init --bare` / `set HEAD` / existence,
  `remote_wiring`); `io::git` mutations (clone, add/set/remove remote, fetch, ff-merge,
  ff-update). `gr create` (bare home + wire + push, refuses if it exists), `gr clone`
  (into-a-root only, drops the clone-minted `origin`), `gr sync` (easy-push ahead, ff-pull
  behind on a clean tree, `-i` confirm, `-a`, `--dry-run`) — all audited. Verified live against
  tenx (create→sync→clone round-trip) + hermetic sync tests.
- **Status UX ([ADR-0014](adr/0014-status-ux-lifecycle-and-repo-detail.md)):** `gr status` is
  now home-aware — a **lifecycle** column (`linked`/`local-only`/`home-only`/`?`), `home-only`
  repos as rows, a **`+N⚠`** "others need attention" hint, and `--offline` + graceful
  degradation. **`gr status <repo>`** (positional) is the all-branches **detail view** with a
  `sync`-action column, resolving by home *or* directory name (so `gr status omarchy-setup`
  finds `USCourts_setup`) and listing home-only branches via one `ls-remote`. Verified live.
  *(`gr homes` is now superseded by `status`'s lifecycle column; kept as a quick diagnostic.)*
- Gates green: build, `clippy -D warnings`, `cargo test` (**58 tests**: core 21 · io 15 ·
  cli 20 · render 2); coverage ~76% line (pure `core` 98–100%; the SSH-execution paths in
  `server`/lifecycle `create`/`clone` are live-verified, not hermetic).

**Not yet:** *mandatory* FIPS (tenx-side `sshd`/crypto-policy — the deferred tier); the
operational item of keeping tenx awake/reachable at day's end; a future GUI (Tauri).

**Recently done:** a **`[backup]` server + `Bkp` column** in `gr status` — per-repo backup
presence (`ok`/`miss`/`?`) when a second home server is configured ([ADR-0015](adr/0015-backup-server-presence-column.md));
the real backup path to tenx (the `create`→`sync`→`clone` round-trips push live);
**`gr status --json`** (machine-readable output, ADR-0006); **CI supply-chain + coverage
gates** — `cargo-vet` (`supply-chain/` baseline) + a CycloneDX SBOM artifact +
`cargo llvm-cov --fail-under-lines 70`; `gr homes` retired into a `status` alias.

**Done since:** integration tests (`assert_cmd`, 8 hermetic cases); `kani` proofs written
**and verified green** (3/3); gix/ADR-0003 deviation reconciled ([ADR-0010](adr/0010-system-git-for-local-reads.md));
**CI live & green** per [ADR-0011](adr/0011-ci-fast-gates-plus-kani-every-push.md)
(`.github/workflows/ci.yml` + `deny.toml`); **SSH transport wired & FIPS fail-closed
verified** ([ADR-0009](adr/0009-ssh-transport-aliases-mdns-hostkey-pinned.md); steps in
[SETUP.md](SETUP.md)).

_(The earlier gix/ADR-0003 deviation is resolved — [ADR-0010](adr/0010-system-git-for-local-reads.md)
adopts system `git` for reads too; the code already matches.)_

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

**Backend decision ([ADR-0010](adr/0010-system-git-for-local-reads.md), superseding
[0003](adr/0003-git-backend-hybrid.md)):** use **system `git` for both local reads and
network ops** (`fetch`/`push` + `merge-tree`). Since `git` ≥ 2.38 is already mandatory,
this adds zero supply-chain surface, matches the user's exact git config, and keeps one
code path. No C in the trust base (`git2`/libgit2 still rejected), the pure `core` parses
all read output (ADR-0002), and **all transport crypto still funnels through one
chokepoint** (`fetch`/`push` only) — exactly what Path A enforces and audits.

No telemetry. No network access except the explicit, user-invoked push.

---

## 4. Command surface

### `gr status` (default command)
Renders a table over the configured repos (discovered within the configured roots; see §5).
Columns:

| Column | Meaning | Source |
|---|---|---|
| Repo | dir name (home name for home-only rows) | discovery / inventory |
| Life | lifecycle: `linked` / `local-only` / `home-only` / `?` (ADR-0012/0014) | inventory |
| Bkp | backup presence: `ok` / `miss` / `?` — only when `[backup]` is set (ADR-0015) | backup inventory |
| Branch | current branch (or detached) | `HEAD` |
| Staged / Unstaged / Untracked / Cf | index vs worktree vs untracked vs conflicts | `git status --porcelain=v2 -z` |
| Per-remote `↑ahead / ↓behind` | commits local-vs-remote-tracking, per configured remote | rev-list left-right |
| ⚠ | `+N` other branches needing attention (ADR-0014) | per-branch classification |

- **Merge difficulty** (`new`/`diverged`/`CONFLICT`) is detected *without touching the
  working tree* via `git merge-tree --write-tree`, which reports conflicts directly.
- Color: clean = dim, ahead-only = green, behind/diverged = yellow, conflict/dirty = red.
- **Scope:** default = current branch per repo; `-a` / `--all-branches` = one row per
  local branch. **`gr status <repo>`** = one repo, all branches, with the `sync` action.
- Flags: `--remote <name>` (limit columns), `--offline` (skip server query), `--json`
  (machine output), `--no-color`. *(`--dirty-only` from ADR-0006 is not yet built.)*

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

### Next increment — repo lifecycle & home-aware status

Decided in [ADR-0012](adr/0012-home-inventory-server-side-bare-repos.md) /
[0013](adr/0013-lifecycle-commands-create-clone-sync.md) /
[0014](adr/0014-status-ux-lifecycle-and-repo-detail.md). **Status: all three built & verified
live — ADR-0012 (home inventory, `gr homes`), ADR-0013 (`gr create`/`clone`/`sync`), and
ADR-0014 (home-aware `gr status` + `gr status <repo>` detail). Increment complete.** The
starting point: `gr` once only saw **local**
working copies; this increment teaches it about the **bare "home" repos** on tenx too, so a
repo becomes *a name with up to two presences* — **local** (a working copy under a root) and
**home** (`/data/git/<name>.git`) — giving each a lifecycle state:

| State | Meaning | Verb |
|---|---|---|
| `local-only` | working copy here, no bare home yet | `gr create` |
| `home-only`  | bare home on tenx, never cloned here | `gr clone` |
| `linked`     | both exist → look at per-branch drift | `gr sync` |

Identity is the **home name** (derived from the `data` remote URL, so `USCourts_setup` ↔
`omarchy-setup` just works); `data`/`data-lan` dedupe to one home (ADR-0012).

- **`gr create [name]`** — init the bare home on tenx, set its `HEAD`, wire `data`/`data-lan`,
  push. Refuses if a home already exists. Audited. **Decided next ([ADR-0016](adr/0016-create-provisions-full-fleet-topology.md), not yet implemented):**
  with a `[backup]` configured, `create` also installs the primary's `post-receive` hook and
  creates + hardens the **backup** home, so a new repo is **redundant** (on both boxes) from one
  command — not merely present on the primary. Surfaced by dogfooding the companion home-fleet
  onboarding, where `create` left repos with `Bkp miss` until the backup home was hand-made.
- **`gr clone <name> [dir]`** — clone a home-only repo; target **must land inside a
  configured root** (default `roots[0]/<name>`) so it's auto-discovered, else `gr` lists the
  roots + how to add one and stops. Drops the clone-minted `origin` (kept cloud-only). Audited.
- **`gr sync [repo...]`** — reconcile **easy work only**: easy-push ahead/new, **ff-pull
  behind on a clean tree**, diverged/CONFLICT reported never forced. Default prints + audits
  each action; `-i/--interactive` confirms per branch; `-a` = full all-branches run;
  `--dry-run` previews. (A superset of `push` — adds the pull direction; the one place `gr`
  writes a working tree, gated to clean + true fast-forward.)
- **`gr status`** gains a **lifecycle** cell, shows `home-only` repos as rows, and a compact
  **`+N⚠`** "others need attention" hint (count of *other* branches ahead/behind/diverged/
  local-only/home-only — so a clean current branch can't hide un-backed-up work).
  **`gr status <repo>`** (positional) is the one-repo, all-branches **detail view** — works
  for local *or* home-only repos and previews what `gr sync` would do per branch.
- Reads **degrade** when tenx is unreachable (home cells `?`, exit 0); `--offline` skips the
  network. Lifecycle commands require the server and fail loudly (ADR-0012 §5).

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

[server]                 # home-repo inventory (ADR-0012) — enables lifecycle/home-aware status
root = "/data/git"       # where the bare repos live on tenx
# aliases = ["tenx-lan", "tenx-ts"]   # default: reuse transport.order (LAN → Tailscale)

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
- ✅ **`kani`** — 3 harnesses in `core/src/proofs.rs` (cfg-gated; no dep added) over the
  integer decision logic: the **"easy push ⇒ behind == 0"** safety invariant, `classify`
  totality, and the easy-push decision table. **Verified green** (`cargo kani -p
  git-redundancy-core` → 3/3, 0 failures). (`rfc3339_utc` is deliberately *not* a Kani
  target — `format!` is too costly to model-check; proptest-covered instead.) Requires
  `rustup`, not the Arch `rust` package (see [TROUBLESHOOTING](TROUBLESHOOTING.md)); CI runs
  it per [ADR-0011](adr/0011-ci-fast-gates-plus-kani-every-push.md).
- ✅ **Integration** with `assert_cmd` + `tempfile`: 8 hermetic cases run the actual binary
  and assert push/status behavior (new-branch, dry-run, fast-forward, up-to-date, failover,
  diverged-skip, dirty-warn, non-zero exit on failure).
- ✅ **CI quality + supply-chain gates** (`.github/workflows/ci.yml`, ADR-0011/0004): `fmt
  --check`, `clippy -D warnings`, `cargo test`, and `cargo-deny` (licenses/bans/sources/
  advisories) on every push; Kani in a separate cached job; a **coverage gate** (`cargo
  llvm-cov --fail-under-lines 70`); and a **supply-chain job** — `cargo vet --locked`
  (`supply-chain/` baseline) + a CycloneDX SBOM artifact. See [DEVELOPMENT.md](DEVELOPMENT.md).
- ⬜ Still open: ongoing `cargo-vet` audits as deps change (baseline is exemptions for now).

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
- [x] ~~**Home-aware lifecycle & status** (next increment)~~ → settled in
      [ADR-0012](adr/0012-home-inventory-server-side-bare-repos.md) (home inventory, identity
      by home name, `[server]` config, graceful degradation),
      [0013](adr/0013-lifecycle-commands-create-clone-sync.md) (`create`/`clone`/`sync`, the
      ff-pull-on-clean-tree extension of ADR-0006) and
      [0014](adr/0014-status-ux-lifecycle-and-repo-detail.md) (lifecycle column, `+N⚠`
      indicator, positional `gr status <repo>` detail). Design locked; implementation pending.

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
