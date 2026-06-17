# ADR-0009: SSH transport via host aliases (mDNS, host-key pinned, FIPS algorithms enforced here)
- Status: Accepted
- Date: 2026-06-17
- Deciders: Ryan

## Context
The git remotes pointed at raw addresses: `data-lan` → `ssh://…@192.168.1.54/…`, `data` →
`ssh://…@100.107.98.89/…`. The hardcoded LAN IP proved fragile — when `tenx-rltec` slept it
dropped off `.54`, killing the remote — and a raw `192.168.1.x` is unsafe on a foreign
network using the same range (it could silently reach a stranger's host). Separately,
ADR-0005 (FIPS Path A) left *where to enforce the approved SSH algorithms* open.

Investigation (this box, Omarchy): Avahi is active, `nss-mdns` is wired into
`nsswitch.conf`, and `tenx-rltec.local` resolves to the LAN IP — so **mDNS works with no
new client setup**. Caveat from `infra-notes/verify-zscaler.sh`: with the Zscaler tunnel
up, Avahi can publish/resolve the **tunnel** address (`100.64.x` on `zcctun0`) instead of
the LAN IP, making `.local` intermittently wrong. (Recent Zscaler resolver work — see
`resolved-zscaler.conf` — may have reduced this; treated as a known, documented failure
mode rather than a blocker.)

## Decision
Stop putting addresses or crypto policy in git remotes. Define **SSH `Host` aliases** in
`~/.ssh/config` and point remotes at the alias name. This is also the single place the
ADR-0005 FIPS-approved algorithm set is enforced.

```sshconfig
Host tenx-lan
    HostName tenx-rltec.local          # mDNS (works today; see troubleshooting if Zscaler breaks it)
    User randallard
    HostKeyAlias tenx-rltec            # stable host-key identity regardless of address
    StrictHostKeyChecking yes          # fail closed if the key ever mismatches
    # ADR-0005 Path A — FIPS-approved algorithms, fail-closed:
    KexAlgorithms ecdh-sha2-nistp256,ecdh-sha2-nistp384,ecdh-sha2-nistp521
    Ciphers aes256-gcm@openssh.com,aes128-gcm@openssh.com,aes256-ctr,aes128-ctr
    MACs hmac-sha2-512-etm@openssh.com,hmac-sha2-256-etm@openssh.com
    PubkeyAcceptedAlgorithms ecdsa-sha2-nistp256,ecdsa-sha2-nistp384,ecdsa-sha2-nistp521,rsa-sha2-512,rsa-sha2-256

Host tenx-ts
    HostName tenx-rltec                # Tailscale MagicDNS (100.107.98.89)
    User randallard
    HostKeyAlias tenx-rltec            # same machine, same pinned key
    StrictHostKeyChecking yes
    # …same FIPS algorithm block…
```

- Remotes become `ssh://tenx-lan/data/git/<repo>.git` (LAN) and
  `ssh://tenx-ts/data/git/<repo>.git` (Tailscale). `gr`'s transport failover (ADR-0006) is
  "try `tenx-lan`, fall back to `tenx-ts`."
- **`HostKeyAlias tenx-rltec`** makes host-key verification use one stable identity for both
  paths and any address — so the pin survives mDNS vs IP and LAN vs Tailscale.
- **Host-key pinning + `StrictHostKeyChecking yes`** is what makes raw-IP/`.local` ambiguity
  safe: a stranger at the same address has a different key and the connection aborts. This
  replaces the "mDNS fails closed" property with a stronger cryptographic one.
- **DHCP reservation** (router admin available): pin `tenx-rltec` to a fixed `192.168.1.x`.
  This stabilizes the LAN IP and is the documented fallback `HostName` when Zscaler breaks
  mDNS (see `docs/TROUBLESHOOTING.md`). Record the reserved IP there once set.

## Consequences
- Address + crypto policy live in one place; repo remotes never change when the network does.
- Answers ADR-0005's open question: **FIPS algorithms are enforced in these `Host` blocks**,
  and `gr` can verify the negotiated algorithms match before pushing.
- mDNS gives zero-setup convenience now; if Zscaler breaks `.local`, the fix is a one-line
  `HostName` swap to the reserved IP (host-key pin still holds) — documented, not surprising.
- Requires pinning `tenx-rltec`'s host key into `known_hosts` once, and confirming tenx's
  `sshd` offers the approved algorithms (Omarchy/OpenSSH 10.x — expected fine).
- Orthogonal reliability risk remains: if `tenx-rltec` is asleep, no addressing helps —
  tracked in troubleshooting.
