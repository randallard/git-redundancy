# 2026-06-18 — Doc staleness sweep + the "no [server]" message fix

**Documents:** commit `a3ff33e` ("doc updates / one message updated in code"). Follows
`15a9ba2` (journal for the `--json`/homes/supply-chain commit). Fifth and last entry today —
housekeeping after the feature work, triggered by a real first-run snag.

## What prompted it

Running `gr status` on the real config printed `(server unreachable …)` even though
`ssh tenx-lan` worked fine. The cause wasn't reachability at all: the live config had **no
`[server]` block**, so `gr` never contacts the server — but the code reported that case with
the *unreachable* message. Two problems fell out of that:

1. The message conflated "not configured" with "unreachable".
2. The **config examples in README.md and SETUP.md both omitted `[server]`** — so anyone
   following the docs would reproduce exactly this, with no hint why the lifecycle column was
   blank.

## What landed

- **Message fix** (`crates/cli/src/main.rs`) — `gr status` now distinguishes three states:
  no `[server]` configured (with a one-line "add this to enable it" hint), `[server]` set but
  unreachable, and `--offline`. Only the middle one says "unreachable".
- **Doc staleness sweep** — the feature work (ADR-0012→0014, `--json`, the gates) had
  outrun the prose:
  - **README** — Status/Highlights/Usage rewritten for the lifecycle commands, the
    home-aware status table (`Life` + `+N⚠` columns, the `gr status <repo>` detail), and
    `--json`; **`[server]` added to the Configure example**.
  - **SETUP.md §5** — added the `[server]` block (the gap that reproduced the snag) and a
    `gr sync --dry-run` verify step.
  - **TROUBLESHOOTING.md** — a new top entry separating no-`[server]` / unreachable /
    `--offline`, including the BatchMode gotcha (an interactive `ssh` working isn't enough;
    `gr` connects with `-o BatchMode=yes`, so the key must be agent-loaded or passphrase-less).
  - **PROGRESS.md §4** — the `gr status` column/flag table predated ADR-0014; added `Life`/
    `⚠`/the detail view/`--offline`/`--json`, and flagged `--dirty-only` (ADR-0006) as not
    yet built.

## Note

Doc-only except the one message string, so it rode in a single commit. The installed `gr` in
`~/.cargo/bin` is still the pre-fix binary until a `cargo install --path crates/cli --force`;
the fix is verified against a fresh `target/debug` build.

## Next

Unchanged from the last entry: the remaining PROGRESS items are mostly ops on tenx (keep it
awake; record the DHCP reservation) and the deferred mandatory-FIPS tier; the Tauri GUI is
unblocked by `--json` whenever it's wanted. Adding the `[server]` block to the live config is
the one outstanding *use* step to turn the lifecycle features on.
