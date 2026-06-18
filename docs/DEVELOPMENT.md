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

## What CI runs

Per [ADR-0011](adr/0011-ci-fast-gates-plus-kani-every-push.md), every push runs the **fast
gates** (`fmt --check`, `clippy -D warnings`, `cargo test`, plus `cargo-deny` for
licenses/bans/sources/advisories) and **Kani** in a separate cached job. Coverage is not yet
a CI gate — it's a local tool for now.
