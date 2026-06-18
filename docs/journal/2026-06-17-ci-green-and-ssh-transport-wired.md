# 2026-06-17 — CI green & SSH transport wired to tenx

**Documents:** the milestone since the [tests/proofs/CI entry](2026-06-17-tests-proofs-and-ci.md)
(`971e859`): CI implemented and **green** (`15c4230` fmt, `66f6dea` CI workflows,
`d532f24` progress), plus the SSH transport wiring to `tenx-rltec` — which is machine-local
environment work, now captured reproducibly in `docs/SETUP.md` + `docs/examples/tenx.conf`
(added in this commit). Fourth entry today.
**Status:** the end-to-end backup path is wired and FIPS-verified; the **first real
`gr push` to tenx has not run yet** (paused at that step).

## What happened

- **CI is live and green** (ADR-0011): fast gates (`fmt`/`clippy -D warnings`/`test` +
  `cargo-deny`) and a separate cached Kani job, both on every push. `cargo-deny` needed one
  documented `ignore` (`RUSTSEC-2024-0370`, the unmaintained build-time `proc-macro-error`
  via `tabled`); licenses/bans/sources clean.
- **SSH transport wired** (ADR-0009 / ADR-0005 Path A): `~/.ssh/config.d/tenx.conf` with
  `tenx-lan` (mDNS) + `tenx-ts` (Tailscale) aliases — FIPS-approved KEX/cipher/MAC, forced
  ECDSA host key, `HostKeyAlias tenx-rltec`, `StrictHostKeyChecking yes`. Pinned tenx's
  ECDSA host key, repointed the 4 repos' `data-lan`/`data` remotes to the aliases, and wrote
  the real `~/.config/git-redundancy/config.toml`. `gr status` runs clean through it.

## FIPS fail-closed — proven

Three checks against the live host:
- **Unconstrained** (no alias), tenx negotiates **non-FIPS**: `mlkem768x25519-sha256` KEX +
  `ssh-ed25519` host key.
- **Through `tenx-lan`**: `ecdh-sha2-nistp256` + `ecdsa-sha2-nistp256` — the config changes
  the outcome on the same host.
- **Forcing `3des-cbc`** → *"no matching cipher found"*: ssh refuses rather than downgrading.
  tenx's offer list even *includes* `chacha20-poly1305` (non-approved) — yet the alias picked
  `aes256-gcm`, i.e. it actively declines the weaker cipher the server would serve.

## Decisions / honest gaps

- **ECDSA host key pinned** (FIPS-traditional) over tenx's ed25519, via `HostKeyAlgorithms`.
- **Client auth key is ed25519** (FIPS 186-5) — `PubkeyAcceptedAlgorithms` left at default to
  avoid locking out the only key; documented as a minor asymmetry.
- **Enforcement is client-side** (strong default, `-o`-overridable). `git`/`gr` don't
  override, so their path is FIPS-only + fail-closed. *Mandatory* enforcement (tenx `sshd` /
  system crypto-policy) is the still-deferred validated/mandatory tier from ADR-0005.
- Cleaned up a junk `known_hosts` line I introduced via an ssh-keyscan banner; the SETUP.md
  pin command now guards against it.

## Next

1. **Run the first real `gr push`** to tenx (the queued step).
2. `local-notes` has uncommitted/untracked files — decide whether to commit before
   relying on the backup (a push won't capture them).
3. Optional later: tenx-side `sshd`/crypto-policy for *mandatory* FIPS enforcement.
