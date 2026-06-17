# ADR-0004: FISMA-High–aligned practices, not a certification claim
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
The goal was stated as "FISMA High if possible." FISMA categorizes *information systems*
(FIPS 199) and applies the NIST SP 800-53 **High baseline** to an **authorized boundary**
with an ATO and continuous monitoring. A local CLI that pushes a user's own repos over
their own LAN/Tailscale is **not** such a system boundary. Claiming the binary "is FISMA
High" would be inaccurate — that status is organizational, not a code property.

## Decision
Adopt the High-baseline **engineering practices** without claiming the **status**:

| 800-53 family | git-redundancy practice |
|---|---|
| SI (integrity) | `#![forbid(unsafe_code)]`; input validation; `cargo-audit` in CI |
| CM (config mgmt) | pinned `Cargo.lock`; `cargo-deny` (license + source allowlist); SBOM; reproducible build |
| SR (supply chain) | `cargo-vet`; minimal deps; optional vendoring |
| AU (audit) | append-only, timestamped audit log of every push (what / where / result) |
| AC (access) | least privilege: configured repos only; **never auto-commit**; explicit remotes |
| SC-13 (FIPS crypto) | see ADR-0005 |

No telemetry. No network except the explicit, user-invoked push/fetch.

## Consequences
- Honest posture: real, demonstrable assurance without a false compliance label.
- If git-redundancy ever enters a real authorization boundary, these practices map directly onto
  control evidence (the SSP/POA&M work becomes incremental, not from-scratch).
- Some CI weight (deny/audit/vet/SBOM) and discipline (locked deps) is now mandatory.
