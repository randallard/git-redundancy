# Wiring a machine to back up to tenx

Reproducible steps to point a working machine's repos at the bare-repo host
(`tenx-rltec`) over the FIPS-enforced transport. The *design* and *why* live in
[ADR-0009](adr/0009-ssh-transport-aliases-mdns-hostkey-pinned.md) and
[ADR-0005](adr/0005-fips-crypto-path-a-enforce-approved-algorithms.md); this is the
"do it" checklist. Gotchas are in [TROUBLESHOOTING](TROUBLESHOOTING.md).

> Replace `tenx-rltec` / `randallard` / repo paths with your own. The SSH alias file has
> no secrets; the host-key *pin* goes in `~/.ssh/known_hosts`.

## 1. Reachability

On the home LAN, `tenx-rltec.local` should resolve (mDNS via Avahi) and ping. Off-LAN, the
`tenx-ts` alias uses Tailscale. If `.local` misbehaves under Zscaler, fall back to the
reserved LAN IP — see TROUBLESHOOTING.

## 2. SSH transport (FIPS-enforced aliases)

Ensure `~/.ssh/config` has `Include config.d/*.conf` near the top, then install the alias:

```bash
mkdir -p ~/.ssh/config.d
cp docs/examples/tenx.conf ~/.ssh/config.d/tenx.conf   # edit hostnames/user to taste
```

## 3. Pin tenx's ECDSA host key (under the HostKeyAlias name)

We force an ECDSA host key (FIPS), so pin *that* key under the alias name `tenx-rltec`.
The `awk` guard avoids capturing ssh-keyscan's banner line:

```bash
ssh-keyscan -t ecdsa tenx-rltec.local 2>/dev/null \
  | awk 'NF>=3 && $1=="tenx-rltec.local"{$1="tenx-rltec"; print}' >> ~/.ssh/known_hosts

# Verify the fingerprint matches tenx before trusting it:
ssh-keygen -lf <(ssh-keyscan -t ecdsa tenx-rltec.local 2>/dev/null)
# expect ECDSA SHA256:PTPAcg55PAfGxXV6/hUqiDdfXGl3SKxJNLWGtqby8p8
```

Confirm strict, FIPS-only connect works:

```bash
ssh -o ControlPath=none tenx-lan 'echo OK $(hostname)'   # -> OK tenx-rltec
```

## 4. Repoint the git remotes at the aliases

For each repo whose `data-lan` points at a raw host, swap to the aliases (keeps the
server-side path):

```bash
for repo in /data/Development/*/; do
  git -C "$repo" remote get-url data-lan >/dev/null 2>&1 || continue
  path=$(git -C "$repo" remote get-url data-lan | sed -E 's#^ssh://[^/]+##')  # /data/git/<repo>.git
  git -C "$repo" remote set-url data-lan "ssh://tenx-lan${path}"
  git -C "$repo" remote get-url data >/dev/null 2>&1 \
    && git -C "$repo" remote set-url data "ssh://tenx-ts${path}"
done
```

## 5. git-redundancy config

`~/.config/git-redundancy/config.toml` (config-first — list the repos to back up):

```toml
repos = [
  "/data/Development/api-server",
  "/data/Development/web-frontend",
  "/data/Development/local-notes",
  "/data/Development/infra-notes",
]
default_remotes = ["data-lan", "data"]

[transport]
auto = true
order = ["data-lan", "data"]

# The bare-repo home on the server. Required for the lifecycle column in `gr status`
# and for `create` / `clone` / `sync`. Omit it and gr stays local (lifecycle shows `?`).
[server]
root = "/data/git"
aliases = ["tenx-lan", "tenx-ts"]
```

## 6. Verify

```bash
gr status                     # table over the aliases, with the lifecycle column
gr push --dry-run             # preview; changes nothing, not audited
gr push                       # back up easy, committed work (LAN-first failover)
gr sync --dry-run             # preview the two-way reconcile (push + fast-forward)
```

### Optional: prove FIPS is fail-closed

```bash
# Unconstrained (no alias) — tenx will offer non-FIPS (mlkem/ed25519):
ssh -o ControlPath=none -o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no \
    randallard@tenx-rltec.local -v true 2>&1 | grep -E 'kex: algorithm:|Server host key:'
# Through the alias — FIPS (ecdh-nistp / ecdsa):
ssh -o ControlPath=none tenx-lan -v true 2>&1 | grep -E 'kex: algorithm:|Server host key:'
# Demand a non-approved cipher — must be refused, never downgraded:
ssh -o ControlPath=none -o Ciphers=3des-cbc tenx-lan true 2>&1 | tail -1
```

## Notes / known asymmetries

- **Client auth key is ed25519** (FIPS 186-5). `PubkeyAcceptedAlgorithms` is left at default
  so it keeps working; transport + host key are traditional-FIPS (ecdh-nistp / aes-gcm /
  ecdsa). For full alignment, add an ecdsa/rsa client key and restrict it.
- **Enforcement is client-side** (a strong default; `-o` can override on a manual `ssh`).
  `git`/`gr` don't override, so their path is FIPS-only and fail-closed. *Mandatory*
  enforcement would be tenx-side `sshd` config or a system crypto-policy (the deferred
  validated/mandatory tier in ADR-0005).
