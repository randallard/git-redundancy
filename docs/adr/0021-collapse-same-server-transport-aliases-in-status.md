# ADR-0021: Collapse same-server transport aliases into one status column
- Status: Accepted
- Date: 2026-07-04
- Deciders: Ryan

## Context
`gr status` renders one sync column **per remote** in the shown set. On the real fleet the shown
set is `default_remotes = ["data-lan", "data"]` (config.rs:200) — but those two remotes are *not*
two destinations. They are two **transports to the same server**: `data-lan` over the LAN,
`data` over Tailscale. The config already says so explicitly: `transport.auto = true` with
`order = ["data-lan", "data"]` means "interchangeable paths to the *same* server — push tries them
in order until one succeeds, so you back up once, preferring the LAN" (config.rs:55–59).

The result is a **visual and semantic doubling**: every repo shows a `data-lan` column *and* a
`data` column for one physical home. Three forces make this more than cosmetic:

- **`push` already collapses; `status` doesn't.** `gr push` with `auto = true` pushes **once** via
  the first transport that works (failover), because one push = backed up. So the tool's *action*
  model treats the group as one destination, but its *reporting* model shows two. Status and push
  disagree about how many things there are to back up.
- **The fleet row already collapses — only the columns lag.** `presence.rs::join_presences` keys on
  the **home name**, so `data`/`data-lan` already resolve to a single lifecycle row
  (presence.rs:68–71). The per-remote columns are the one place the two-transports-one-server truth
  isn't yet applied.
- **It manufactured a real false alarm (the ADR-0019 §history).** Because a failover push only
  moved the *used* alias's tracking ref, the sibling column showed a phantom `↑n` until a manual
  fetch — read as "this repo isn't backed up" when it was. ADR-0019 (fetch-before-columns) makes
  both aliases now read *truthfully*, but it left the redundancy in place: two columns that, when
  healthy, always say the same thing, and whose only distinct states are transport-diagnostic
  ("LAN reachable but Tailscale isn't"), not backup-state. Collapsing removes both the redundancy
  and the residual window where the two can transiently disagree.

What the collapse must **not** do: hide genuinely independent destinations. The `[backup]` server
(ADR-0015, the `Bkp` column) and a cloud `origin` are **not** in the transport group — they are
separate places the work does or doesn't exist, and must keep their own columns. The collapse is
scoped to *aliases the config declares interchangeable*, nothing else.

## Decision
When `transport.auto = true`, **render the transport-alias group as a single logical column** in
`gr status` (fleet and detail views), rather than one column per alias.

1. **Grouping key: the declared transport group.** The set `transport.order` (with `auto = true`)
   is one logical destination. Remotes not in that group — `backup`, `origin`, anything the user
   added — remain their own columns. `auto = false` disables the collapse entirely (the user has
   said the remotes are independent). `--remote <x>` still pins exactly one alias (unchanged).
2. **Column label: the logical/home name, not a transport alias.** Prefer the home name; absent
   that, the group's canonical member (the last/most-portable entry, conventionally `data`). The
   header reads as the *destination*, not the wire.
3. **Cell value: the best (most-backed-up) state across the group's aliases.** Since any one
   successful push means the server has the work, the collapsed cell is the *max* over the aliases:
   if any alias reads `ok`, the cell is `ok`; it degrades to `↑n`/`stale`/`?` only when **every**
   alias does. This is the correct backup semantics and is intentionally optimistic about
   *backup state* while staying honest — a genuinely un-pushed branch reads `↑n` on all aliases and
   still surfaces. (With ADR-0019's fetch the aliases normally agree; the max rule only matters in
   the transient-disagreement window, and resolves it in the safe-and-true direction.)
4. **Diagnostics stay reachable via `--by-remote`.** A new `--by-remote` flag expands back to the
   pre-ADR per-alias columns, for when you *do* want to see "LAN ok, Tailscale unreachable." The
   collapsed view is the default because the everyday question is "is it on the home server?", not
   "which wire got it there."
5. **The collapse is a pure-core function.** Given the per-alias cells and the transport group,
   producing the collapsed cell is a pure fold — it lives in the functional core (ADR-0002),
   fully unit-testable, with the imperative shell only supplying the fetched cells and the group.

## Consequences
- **Status and push finally agree on how many destinations exist.** One home = one column, matching
  what `push` already does and what the fleet row already shows. The everyday `gr status` gets
  narrower and stops implying there are two places to back up to when there's one.
- **The last residence of the "doubled / phantom ↑n" symptom is gone.** ADR-0019 made the two
  columns *truthful*; this makes them *one*, so even a transient tracking-ref skew between siblings
  can't render as a scary second column. Together they close the issue end to end.
- **A transport outage is now opt-in to see, not in your face.** If Tailscale is down but LAN is up,
  the default collapsed cell still says `ok` (correctly — you're backed up), and `--by-remote`
  reveals the degraded transport. Trade-off accepted: the default optimizes for the backup question;
  the diagnostic is one flag away. (A future refinement could add a subtle "1 of 2 transports"
  annotation without un-collapsing — deferred.)
- **`auto = false` users are unaffected.** They've declared their remotes independent, so nothing
  collapses; they keep a column each.
- **New surface to test:** the pure collapse fold (max-over-aliases, label selection, group vs
  non-group membership) as unit tests; a render test that `["data-lan","data"]` → one `data` column
  by default and two under `--by-remote`; and that `backup`/`origin` never fold in. JSON output
  (`statusjson`) gains the collapsed shape by default — a compatibility note for any consumer that
  keyed on per-alias columns (there are none today; `gr` is the only consumer).
- **Does not touch push, fetch, or the ahead/behind primitive.** Purely a status
  model/render change on top of ADR-0019's now-fresh refs.
