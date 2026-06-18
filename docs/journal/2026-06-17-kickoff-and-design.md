# 2026-06-17 — Kickoff & design

**Documents:** initial commit `59dcf06` ("initial commit") — the `docs/` design set.
**Status:** design complete, no code yet.

## Where this came from

The seed was a real workflow need: after a day working on the laptop (`waed-7561`, Omarchy),
back up all working repos to their bare "home" remotes on `tenx-rltec` (`/data/git/*.git`),
documented in `infra-notes/GIT_REPOS_PLAN.md`. The existing setup had the bare repos and
two remotes (`data` = Tailscale, `data-lan` = LAN) but **no one-shot "push all my working
repos home" tool** — and the `data-lan` remote was a fragile hardcoded IP that had just
broken when the home box slept. That gap became this project: **git-redundancy**, a CLI to
see the state of many repos and push the easy, committed work home.

## What we accomplished

Turned a vague "quick backup command" into a fully-specified, decision-backed design:

- **Scope settled** — two commands: `gr status` (a table: per-repo branch, staged/unstaged/
  untracked, per-remote ahead/behind, and merge difficulty via `git merge-tree`) and
  `gr push` (push only *easy* = fast-forwardable, *committed* work; never auto-commit, never
  force, skip diverged; warn loudly on dirty trees). Both support current-branch and
  `--all-branches` views.
- **Ten ADRs (0000–0009)** capturing every non-obvious choice with its rationale:
  - *Tooling/architecture:* Rust CLI; functional-core / imperative-shell split so the
    provable/testable logic is isolated from IO; hybrid git backend (`gix` for local reads,
    system `git` for all network ops + `merge-tree`), keeping C out of the trust base.
  - *Security, honestly scoped:* FISMA-High *aligned engineering practices*, **not** a
    compliance claim (no ATO/boundary). Researched the FIPS question and found Arch/Omarchy
    **structurally cannot** ship a CMVP-validated module (rolling release vs frozen cert) —
    so we adopted **Path A** (enforce FIPS-approved SSH algorithms, fail-closed, now) with a
    documented graduation path to a validated module via a certified container/host.
  - *Platform & transport:* recorded Omarchy on both ends (its security posture, pros/cons);
    replaced the brittle hardcoded remotes with **SSH `Host` aliases** (`tenx-lan` over mDNS,
    `tenx-ts` over Tailscale), **host-key pinned** for fail-closed safety, with the FIPS
    algorithm block living in the alias — which also answered *where* Path A is enforced.
  - *Config-first:* repos are never auto-discovered globally; roots are explicitly
    configured and repos discovered within them (plus explicit `repos`/`exclude`).
- **Operational docs** — `TROUBLESHOOTING.md` for the known Zscaler-breaks-mDNS gotcha (with
  fix), the "tenx asleep" reliability case, and host-key mismatches. `PROGRESS.md` as the
  living overview.
- **Established this journal** and its conventions (see `README.md`).

## Decisions of note (the "why", briefly)

- **"Provable" was scoped into tiers** rather than promised wholesale: memory safety is free
  in safe Rust; illegal-states-unrepresentable + `proptest` are cheap; `kani` proofs apply
  only to the isolated pure core. That scoping drove the functional-core/imperative-shell
  architecture.
- **We refused to overclaim security.** "FISMA High" and "FIPS-validated" were both walked
  back to what's truthfully achievable on Arch, with the gap documented and a real path
  forward — not hidden.

## Next steps

1. Clone is done; **scaffold the Cargo workspace** (`git-redundancy-core` / `-io` / `-cli`)
   and start on `gr status`.
2. **Stand up the SSH aliases + DHCP reservation + host-key pin** (ADR-0009) so the LAN
   backup works today, independent of the tool.
3. Sort out `tenx-rltec`'s suspend/idle so it stays reachable at day's end.
