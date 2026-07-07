# 2026-07-04 — One server, one column: collapsing the doubled transport aliases

**Documents:** the ADR-0021 implementation (pure `core/collapse.rs` + a display-time fold in
`cli/main.rs`). Follows [ADR-0019](../adr/0019-freshness-before-classification-status-push-fetch.md),
which made the two alias columns *truthful*; this makes them *one*.
**Status:** ✅ implemented, hermetically tested, and verified against the live fleet config
(`--offline`, so no network) — the collapse is display-only, so offline fully exercises it.
Not yet committed (awaiting local verification per the working agreement).

## The itch

`gr status` rendered a `data-lan` **and** a `data` column for every repo — but those are two
transports to the *same* server (acer), declared interchangeable in config (`transport.auto = true`,
`order = ["data-lan", "data"]`). So every repo looked "doubled," and after a failover push the
sibling alias could read a phantom `↑n` (the ADR-0019 history). ADR-0019 fixed the *staleness*;
the redundant second column remained. `push` already treats the group as one destination (it pushes
once via failover); the fleet *row* already collapses on home name (`presence.rs`). Only the
per-remote *columns* still showed the group as two.

## What landed

- **`core/collapse.rs` (pure, ADR-0002).** `collapse_columns(shown, group)` builds a plan: the
  transport group folds into one column labeled by its canonical member (last-in-order that's
  shown → `data`); non-group remotes (`backup`, `origin`) keep their own column in place. `fold`
  reduces a row's per-alias cells with `most_backed_up` — a total order where **`ok` beats a stale
  `↑n`** (same server, so the truthful cell is the one showing the server holding more of our work),
  degrading only when *every* alias agrees. `None` only when every alias is absent (cloud-only repo
  → one `-`, not two). 7 unit tests, incl. the headline `ok`-beats-stale-`↑1` case and
  "a genuine gap still surfaces when all aliases agree."
- **Display-time application (`cli/main.rs`).** Rows are still computed per-alias (fetch + columns
  intact, ADR-0019); the collapse is applied *just before* render/JSON in both the fleet and detail
  paths, so `render.rs`/`statusjson.rs` are untouched. Gated by `collapse_plan`: on by default when
  `transport.auto`, off under the new `--by-remote` flag or an explicit `--remote` (already one
  column).

## Verified

Against the real config, `--offline`:

- default → a single `data` column; `--by-remote` → `data-lan` + `data` both back.
- `--remote data-lan` → one pinned `data-lan` column (no collapse).
- detail view (`gr status <repo>`) collapses too.
- JSON `remotes` header: `["data"]` default, `["data-lan","data"]` under `--by-remote`.
- full suite green (32 core tests incl. the 7 new), `fmt`/`clippy` clean.

## Next

- Commit (pending local sign-off).
- Optional future refinement noted in the ADR: a subtle "1 of 2 transports" annotation on the
  collapsed cell when a transport is down, without un-collapsing — deferred.
- Still open elsewhere: the onboard/repoint/attach paths (ADR-0017/0018/0020) remain implemented
  but not *live*-verified; `gr onboard --dry-run` is the read-only next step.
