# Development — gates, tests, coverage, Kani

How to run the quality checks locally. These are the same gates CI enforces on every push
(fast gates + Kani, per [ADR-0011](adr/0011-ci-fast-gates-plus-kani-every-push.md)), so
running them before you commit keeps CI green.

Prerequisites: a Rust toolchain via [rustup](https://rustup.rs) (not the distro `rust`
package — Kani needs `rustup`; see [TROUBLESHOOTING](TROUBLESHOOTING.md)) and system `git`
≥ 2.38. Run everything from the repo root.

## The fast gates (run before every commit)

```bash
cargo fmt --check                          # formatting
cargo clippy --all-targets -- -D warnings  # lints (warnings are errors)
cargo test                                 # the whole test suite
```

If `fmt --check` complains, apply it with `cargo fmt` (no `--check`).

## Format — `cargo fmt`

```bash
cargo fmt            # reformat in place
cargo fmt --check    # verify only; non-zero exit if anything would change (CI uses this)
```

## Lint — `cargo clippy`

```bash
cargo clippy --all-targets -- -D warnings   # lint lib + tests + bins, warnings fail
cargo clippy --fix                          # auto-apply the machine-applicable suggestions
```

`--all-targets` matters: it lints test and example code too, not just the library.

## Tests — `cargo test`

```bash
cargo test                          # all tests across core, io, cli
cargo test -p git-redundancy-core   # one crate (here: the pure core + proptests)
cargo test --test cli               # just the cli integration tests (assert_cmd)
cargo test presence                 # only tests whose name matches a substring
cargo test -- --nocapture           # don't swallow println!/stdout from tests
```

What's covered:

- **Unit + property tests** (`proptest`) in `git-redundancy-core` — classification,
  "easy push", the porcelain parser, the presence join, the sync planner. Proptests are
  ordinary `#[test]`s, so **`cargo test` runs them**.
- **Integration tests** (`assert_cmd` + `tempfile`) in `crates/cli/tests/cli.rs` — run the
  real `gr` binary against hermetic git fixtures (isolated HOME / XDG / git config).
- The **live** tenx round-trips (`create`→`sync`→`clone`, the inventory) are exercised by
  hand against the real server, not in the hermetic suite — they need an actual SSH home.

## Coverage — `cargo llvm-cov`

One-time setup (already done on the dev box):

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
```

Run it:

```bash
cargo llvm-cov --workspace --summary-only          # the per-file + TOTAL table
cargo llvm-cov --workspace --html --open           # full line-by-line report in the browser
cargo llvm-cov --workspace --text | less -R         # annotated source in the terminal
cargo llvm-cov --workspace --lcov --output-path lcov.info   # machine-readable (for a CI gate)
```

Reading the summary: columns are Regions / Functions / **Lines** / Branches, with a `TOTAL`
row. The pure `core` sits at ~98–100%; the lower numbers are the **SSH-execution paths**
(`io/server.rs`, and `create`/`clone` in `cli/src/lifecycle.rs`) that only run against a live
server and are verified by hand instead. `llvm-cov` does its own instrumented build and
re-runs the tests, so the first run is slower than a plain `cargo test`.

> Note: like `cargo test`, `llvm-cov` **includes** the proptests but **excludes** the Kani
> proofs (those are `#[cfg(kani)]`-gated and don't compile under a normal build).

## Formal proofs — `cargo kani`

The safety-critical integer logic (e.g. *a push is only "easy" when not behind*) is proven
with the [Kani](https://model-checking.github.io/kani/) bounded model checker. The harnesses
live in `crates/core/src/proofs.rs`, gated behind `#[cfg(kani)]`, so a normal `cargo
test`/`build` compiles right past them.

One-time setup (needs `rustup`, not the distro `rust` package):

```bash
cargo install --locked kani-verifier
cargo kani setup
```

Run the proofs:

```bash
cargo kani -p git-redundancy-core    # runs every #[kani::proof] harness in core
```

If `cargo kani setup` fails at the toolchain step on Arch (`rustup … No such file or
directory`), you're on the pacman `rust` package — switch to `rustup`; the fix is in
[TROUBLESHOOTING](TROUBLESHOOTING.md).

## Supply chain — `cargo-deny`, `cargo-vet`, SBOM

Dependency hygiene gates (ADR-0004, SR/CM families).

```bash
cargo deny check                          # licenses, bans, sources, advisories (config: deny.toml)
cargo vet                                 # every dependency is audited or exempted (supply-chain/)
cargo cyclonedx --format json --all       # generate a CycloneDX SBOM (*.cdx.json, gitignored)
```

`cargo-vet` keeps its state in `supply-chain/` (committed): `config.toml` lists exemptions
for the current tree, `audits.toml` holds any first-party audits. After adding or bumping a
dependency, `cargo vet` will flag it — re-exempt or audit it (`cargo vet --help`), commit the
`supply-chain/` change. CI runs `cargo vet --locked`. Install the tools with
`cargo install cargo-vet cargo-cyclonedx` (CI uses `taiki-e/install-action` for prebuilt
binaries).

## Coverage gate

**Tiered by testability** ([ADR-0023](adr/0023-coverage-gate-tiered-by-testability.md)) — three
runs, not one blended number. The same three checks CI runs:

```bash
# 1. pure core (ADR-0002) — should stay near-total
cargo llvm-cov --locked -p git-redundancy-core --fail-under-lines 95

# 2. the real quality gate: everything a hermetic test can reach
cargo llvm-cov --locked --workspace --fail-under-lines 80 \
  --ignore-filename-regex '(cli/src/lifecycle\.rs|io/src/server\.rs|io/src/git\.rs)'

# 3. reported, NOT gated — keeps the blended trend visible
cargo llvm-cov --locked --workspace --summary-only
```

Measured 2026-08-13: core **96.37%**, testable surface **84.30%**, whole workspace **65.51%**.

A single blended floor averaged a nearly-fully-covered pure core against SSH orchestration that
cannot be tested hermetically, so it described neither — and the old 58% "temporary" floor hid
the fact that the testable surface was already above the 70% bar that predated it. Splitting
them raised the real bar rather than lowering it.

**The gate-2 exclusion list is exhaustive and closed** — those three files by name, no globs.
Adding a file to it requires a new ADR superseding 0023; that is what stops it becoming a place
to hide untested code. Moving files *out* (by writing an SSH stub — see
`Fixture::install_fake_ssh` in `crates/cli/tests/cli.rs`, which already fakes a home server for
the ADR-0015 tests) is the way to raise gate 2.

## What CI runs

Per [ADR-0011](adr/0011-ci-fast-gates-plus-kani-every-push.md) and ADR-0004, every push runs:

- **fast gates** — `fmt --check`, `clippy --locked -D warnings`, `cargo test --locked`,
  `cargo-deny` (which now also enforces [ADR-0010](adr/0010-system-git-for-local-reads.md) via
  `[bans] deny` on `git2`/`libgit2-sys`/`gix`);
- **kani proofs** — separate cached job;
- **coverage gate** — the three tiered runs above (ADR-0023);
- **supply chain** — `cargo vet --locked` + a CycloneDX SBOM artifact.

Every job carries a `timeout-minutes` ceiling (gates 10, coverage 15, supply-chain 10, kani 20)
against GitHub's 6-hour default. Warm runtimes are 12–60s, so these only trip on a real wedge.

**`--locked` is on every dependency-resolving command.** Without it CI silently re-resolves and
a `Cargo.lock` that no longer satisfies the manifest passes green — which is exactly what
happened for two months. `cargo fmt` is excluded (it rejects the flag and resolves nothing).
Install the binary the same way: `cargo install --path crates/cli --locked`; cargo does **not**
default to it and has no config option to make it default ([rust-lang/cargo#8207](https://github.com/rust-lang/cargo/issues/8207)).
