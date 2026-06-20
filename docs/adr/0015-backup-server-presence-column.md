# ADR-0015: A `[backup]` server and a `Bkp` presence column in `gr status`
- Status: Accepted
- Date: 2026-06-19
- Deciders: Ryan

## Context
The fleet now has two home servers: a **primary** (where clients push) and a **backup** that the
primary replicates to (see the companion `home-fleet` project: primary + controller, hardened
receive-only backup). The end-of-day question grew a second half: not just "is my work pushed to
the home?" but "is the home **also** mirrored to the backup?" Today nothing in `gr` surfaces the
backup side — a repo could be perfectly pushed yet silently absent from the backup box.

A separate on-backup monitor (`fleet-healthcheck.sh`) already checks replication **lag** (refs the
primary has that the backup lacks) and **snapshot freshness**. But that runs on the backup host,
where it can see the filesystem; a client running `gr status` cannot cheaply or honestly observe
ref-level lag or snapshot age across two servers.

## Decision
Add an optional **`[backup]`** server block (same shape as `[server]`: `root` + explicit
`aliases`) and, when it's configured, a **`Bkp`** column in the `gr status` fleet table showing
each repo's **presence** on the backup:

- `ok` — the repo's home exists on the backup too,
- `miss` — it does **not** (a redundancy gap), shown red,
- `?` — the backup is configured but unreachable,
- (no column) — no `[backup]` configured.

It reuses the existing home-listing machinery (the ADR-0012 `ls -d <root>/*.git` over SSH
aliases) — one extra cheap listing, joined against the same home-name identity the lifecycle
column already uses. `--json` carries a per-repo `backup` field; `--offline` skips the query.

**Scope is deliberately presence, not lag.** Replication lag and snapshot freshness stay with
`fleet-healthcheck.sh` on the backup host, which can actually see them. `gr` answers the honest
client-observable question — "is each repo *on* the backup?" — and no more.

## Consequences
- The redundancy gap that matters most (a repo never mirrored to the backup) is now visible at a
  glance, in the table users already read; `miss` in red is hard to miss.
- One extra SSH listing per `gr status` when `[backup]` is set; reuses existing code, no new
  trust surface, and `--offline` opts out.
- `gr` does **not** become a full replication monitor — that boundary is intentional and
  documented, so the table stays fast and the claims stay honest.
- The `Server` config struct now serves two roles (`[server]`/`[backup]`); `backup_enabled()`
  requires explicit `aliases` since there's no per-repo backup remote to derive them from.
