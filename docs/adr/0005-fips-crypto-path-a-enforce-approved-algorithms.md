# ADR-0005: FIPS crypto — enforce approved algorithms now (Path A), graduate to a validated module later
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
The decision was "require FIPS-validated crypto now." Investigation on this machine
(`waed-7561`, Arch Linux) showed that this **cannot be literally satisfied on Arch**, for
structural reasons, not a fixable gap:

- **No FIPS provider present:** `openssl version` → OpenSSL **3.6.2**; `openssl list
  -providers` shows only the Default provider (no FIPS provider).
- **Current SSH to the home box uses non-approved algorithms:** negotiated
  `kex: mlkem768x25519-sha256`, cipher `chacha20-poly1305@openssh.com`. Neither the
  curve25519/ML-KEM hybrid KEX nor ChaCha20-Poly1305 is on the FIPS-approved list for SSH.
- **Rolling release is incompatible with CMVP validation.** A FIPS-validated module is a
  *specific frozen build* tied to a certificate; the validation process "takes a very long
  time, making it impossible to validate every minor release." Arch (and Omarchy, built on
  it) continuously updates past any validated version by design. This is the model, not an
  oversight — so **Arch/Omarchy will not ship an in-distro validated module**, and there is
  no meaningful ETA for "Arch gets FIPS."
- **The validated module is version-pinned and older than Arch's base.** The current
  validated OpenSSL FIPS provider is **3.1.2, CMVP Certificate #4985** (valid to
  2030-03-10), documented compatible with OpenSSL **3.0 / 3.1 / 3.2** base libraries — not
  the 3.6.x Arch ships. Chainguard's **#5102** (2026-01-07) is a rebrand of #4985 with no
  crypto changes. So even pairing Arch's base lib with the certified provider falls outside
  the provider's supported envelope.
- **Omarchy** has no FIPS posture; community hardening (dannymcc/omarchy-hardening) covers
  LLMNR/UFW/login limits, not FIPS.

Conclusion: on Arch we can have **FIPS-approved *behavior*** but **not a *validated
module***. A validated module requires running the crypto-bearing step on a certified
platform (RHEL/Rocky+CIQ, Ubuntu Pro FIPS) or inside a FIPS-validated container image
(Chainguard FIPS), not waiting on Arch.

## Decision
**Path A now, with a documented graduation to a validated module (Path B) later.**

**Plan now — Path A (enforce approved algorithms, fail-closed):**
Because all network crypto funnels through system `git`/OpenSSH (ADR-0003), git-redundancy will
pin and verify the SSH transport to the FIPS-approved set and **refuse to push/fetch over a
non-approved connection** (fail-closed), logging the negotiated algorithms to the audit log
(ADR-0004, AU). Approved set for SSH:

- **KEX:** `ecdh-sha2-nistp256`, `ecdh-sha2-nistp384`, `ecdh-sha2-nistp521`
  (plus `diffie-hellman-group14-sha256` / `group16-sha512` if needed).
- **Ciphers:** `aes256-gcm@openssh.com`, `aes128-gcm@openssh.com`, `aes256-ctr`, `aes128-ctr`.
- **MACs:** `hmac-sha2-256(-etm)`, `hmac-sha2-512(-etm)` (GCM provides its own integrity).
- **Host/pubkeys:** `ecdsa-sha2-nistp256/384/521`, `rsa-sha2-256/512`.
- **Excluded (non-approved):** `chacha20-poly1305@openssh.com`, `curve25519-sha256`,
  `mlkem768x25519-sha256` — i.e. exactly what the link currently negotiates, so this is a
  real change, enforced via an SSH config block git-redundancy manages/verifies.

This gives FIPS-*mode behavior* and a verifiable control today; the underlying module
remains **non-validated** on Arch and that limitation is stated, not hidden.

**Plan later — Path B (validated module), when required:**
Run only the crypto-bearing step on a certified base. Options, in rough order of effort:
1. **FIPS-validated container for the push** — execute `git push`/`fetch` inside a
   **Chainguard FIPS** (OpenSSL 3.1.2, CMVP #5102/#4985) image; the laptop stays Arch.
2. **Certified home server** — put the receiving box (`tenx-rltec`) on **Ubuntu Pro FIPS**
   or **Rocky Linux + CIQ FIPS** so the server side runs a validated module.
3. **Certified client host** — perform end-of-day pushes from a RHEL/Rocky/Ubuntu-Pro-FIPS
   machine when validation is actually required for the data in question.

git-redundancy's design supports this without rework: the transport is a single replaceable
chokepoint, so "run the push in a FIPS container" is a config/wrapper change, not a rewrite.

## Consequences
- Today, on Arch: fail-closed enforcement of FIPS-approved SSH algorithms + audit logging.
  Honest label: **"FIPS-approved algorithms, non-validated module."**
- Requires changing the SSH algorithms currently used to reach the home box (drops
  ChaCha20/curve25519/ML-KEM in favor of NIST P-curves + AES-GCM). Must confirm the server's
  `sshd` offers the approved set (AES-GCM + ecdh-nistp + hmac-sha2 are universally supported).
- True SC-13 (validated module) is explicitly deferred to Path B and is a platform/container
  change, never a "wait for Arch" item.
- Re-evaluate when CMVP validates a provider compatible with OpenSSL 3.6.x, or if the data
  handled ever requires a validated module before then.

## References
- OpenSSL, *README-FIPS* (validated module must be a certified version; build with
  `enable-fips`): https://github.com/openssl/openssl/blob/master/README-FIPS.md
- Red Hat, *The experience of bringing OpenSSL 3.0 into RHEL and Fedora* (validation takes a
  long time; distros ship the validated module separately from the base lib):
  https://www.redhat.com/en/blog/experience-bringing-openssl-30-rhel-and-fedora
- OpenSSL Library, *OpenSSL 3.1.2: FIPS 140-3 Validated* (Cert #4985, valid to 2030-03-10;
  compatible with OpenSSL 3.0/3.1/3.2): https://openssl-library.org/post/2025-03-11-fips-140-3/
- Chainguard, *Chainguard FIPS enters 2026 with OpenSSL 3.1.2* (CMVP #5102, 2026-01-07,
  rebrand of #4985): https://www.chainguard.dev/unchained/chainguard-fips-enters-2026-with-openssl-3-1-2-and-better-cmvp-visibility
- NIST CMVP, OpenSSL FIPS Provider security policy (#4985 / SP 140sp4985):
  https://csrc.nist.gov/CSRC/media/projects/cryptographic-module-validation-program/documents/security-policies/140sp4985.pdf
- OSSL_PROVIDER-FIPS(7) — Arch manual page (the provider exists in docs but is not built/
  validated by default on Arch): https://man.archlinux.org/man/OSSL_PROVIDER-FIPS.7ssl.en
- Omarchy hardening (community; scope is LLMNR/UFW/login limits, not FIPS):
  https://github.com/dannymcc/omarchy-hardening
- CIQ, *FIPS 140-3 compliance for Rocky Linux* (a certified-distro Path B option):
  https://seekingalpha.com/pr/20078635-ciq-announces-fips-140minus-3-compliance-for-rocky-linux-empowering-secure-open-source
- RHEL 9, *Switching RHEL to FIPS mode* (reference for a certified-platform Path B):
  https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/9/html/security_hardening/switching-rhel-to-fips-mode_security-hardening
