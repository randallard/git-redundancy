# ADR-0008: Target OS is Omarchy (Arch-based) on both client and server
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
Both ends of this system run **Omarchy** — the laptop (`waed-7561`, the working-copy
client) and the home git server (`tenx-rltec`, hosting the bare repos at `/data/git`).
Omarchy is DHH's opinionated **Arch Linux**-based desktop distribution (Hyprland + a
curated package set). The OS choice shapes the crypto story (ADR-0005), the security
posture we inherit, and what we must harden ourselves. Recording it so the assumptions are
explicit, since "it's just Arch underneath" has real consequences for a tool whose whole
job is moving repo data over the network.

## Omarchy security strategy & posture (as documented)

**On by default:**
- **Mandatory full-disk encryption (LUKS).** Lost/stolen devices don't expose data — this
  covers *both* the laptop's working copies and the server's bare repos + bundle backups
  at rest.
- **Firewall on by default (UFW).** Blocks all incoming **except SSH (22)** and **LocalSend
  (53317)**; Docker is locked down via `ufw-docker` to prevent accidental container exposure.
- **Signed distribution.** ISOs and packages are cryptographically signed (key
  `40DFB630FF42BCFFB047046CF0134EE680CAC571`, `.sig` alongside releases). Distribution
  infra (ISOs, Omarchy packages, Arch mirror) sits behind **Cloudflare DDoS protection**.

**Package / update model:**
- **Rolling release** on Arch core/extra/multilib **plus the Omarchy Package Repository**;
  patches land immediately via `yay -Syu`. **AUR is available but not enabled by default**,
  keeping provenance tighter.

**Stated philosophy:** optimize for doing "Real Work in the Real World" without
hardware-loss security emergencies — explicitly a **convenience-leaning** distro where, in
the maintainers' framing, "some security defaults get loosened."

**Known soft defaults (community hardening, `dannymcc/omarchy-hardening`):**
- **LLMNR enabled** → local-network name-poisoning exposure.
- (Earlier versions) **UFW configured but not actually running** — now on by default per
  the manual, but worth *verifying* rather than assuming.
- **Permissive login attempts** (10 before lockout).
- Recommended hardening includes: disable LLMNR, confirm UFW active, **restrict SSH to
  Tailscale-only**, drop failed-login limit to 3, enable Git commit signing. The author's
  own caveat: *"you should not rely on automation to secure your system."*

## Decision
Accept **Omarchy on both ends** as the platform, and treat its posture as follows for
git-redundancy:

1. **Lean on what's strong:** LUKS gives at-rest protection for working copies *and* bare
   repos/bundles (complements `GIT_REPOS_PLAN.md`, whose bundle backups currently share the
   `sdb1` disk — LUKS covers *theft*, not *drive failure*; off-drive copy still matters).
2. **Harden the one surface we depend on — `sshd` on the server.** UFW's default lets *all*
   incoming SSH reach `tenx-rltec`, and SSH is exactly our git transport. Restrict `sshd`
   exposure to the **Tailscale interface + home LAN** (per the hardening guidance), so the
   bare-repo server isn't broadly reachable on port 22.
3. **Pin SSH algorithms (ADR-0005 Path A) — consistent on both ends.** Same OS both sides
   means the FIPS-approved set (ecdh-nistp + AES-GCM + hmac-sha2) negotiates identically;
   no cross-platform algorithm mismatch to worry about.
4. **Apply baseline hardening** on both boxes: LLMNR off, confirm UFW running, tighten
   login attempts. (Out of scope for the CLI itself, but a documented prerequisite.)

## Consequences

**Pros:**
- **Rolling release = newest OpenSSH/OpenSSL/git**, so modern algorithms and prompt
  security patches are available immediately (good for Path A's approved-algorithm set).
- **LUKS everywhere** gives strong, free at-rest confidentiality for repo data on both ends.
- **Same OS on both ends** → identical crypto behavior, one mental model, no porting.
- Distro-level **signed packages + Cloudflare** give a reasonable supply-chain/availability
  baseline beneath our own `cargo-deny`/`vet`/SBOM gates (ADR-0004).

**Cons / risks we accept and mitigate:**
- **No FIPS-validated crypto module, ever** — rolling release is structurally incompatible
  with CMVP's frozen-build model (see ADR-0005). This is *the* reason FIPS is Path A, not
  validated. A validated module requires a certified platform/container, not Omarchy.
- **Convenience-leaning defaults** (LLMNR, login limits, broad SSH ingress) require manual
  hardening; nothing about Omarchy is ATO/FISMA-boundary-ready out of the box.
- **No Secure Boot / measured boot** called out in Omarchy's security docs — a gap relative
  to a hardened enterprise baseline; LUKS mitigates at-rest but not a tampered-boot scenario.
- **AUR provenance** if ever enabled — keep it off / treat AUR packages as untrusted.

## References
- Omarchy Manual — Security (LUKS, UFW defaults, signing key, Cloudflare, package model):
  https://learn.omacom.io/2/the-omarchy-manual/93/security
- `dannymcc/omarchy-hardening` (LLMNR, UFW state, login limits, SSH-to-Tailscale, git signing):
  https://github.com/dannymcc/omarchy-hardening — writeup: https://blog.dmcc.io/journal/omarchy-hardening/
- Rolling-release vs FIPS validation rationale: see [ADR-0005](0005-fips-crypto-path-a-enforce-approved-algorithms.md).
