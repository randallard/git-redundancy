# ADR-0000: Record architecture decisions (use ADRs)
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
git-redundancy involves a chain of non-obvious architecture and security choices (language,
git backend, crypto posture). Decided in conversation, the *rationale* would be lost. We
want a durable, reviewable record of each decision and why — and a clear answer to "is
there an official format for this?"

The answer: **ADRs** (Architecture Decision Records, Nygard 2011), commonly written with
the **MADR** Markdown template. ("DAC" = Discretionary Access Control, an access-control
model — not a decision-record format.) There is no government-mandated engineering
decision format; ADR/MADR is the industry de-facto. For security-control decisions we
cite the relevant NIST 800-53 family inline rather than maintaining a separate SSP at
this stage.

## Decision
Keep an ADR log under `docs/adr/`, one file per decision, MADR-lite template (see
`README.md`). ADRs are immutable in substance — supersede rather than rewrite.

## Consequences
- The *why* is preserved and reviewable in-repo, alongside the code it governs.
- Small per-decision overhead; supersession chain instead of edits.
- Decisions touching security controls are traceable to 800-53 families without
  standing up full RMF paperwork prematurely.
