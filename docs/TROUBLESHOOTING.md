# git-redundancy — troubleshooting

Operational gotchas for the repo-backup transport. Design rationale lives in
[`adr/`](adr/README.md); this is the "it broke, now what" doc.

## `data-lan` push fails / `tenx-rltec.local` resolves wrong when Zscaler is up

**Symptom:** pushes to `data-lan` (the `tenx-lan` SSH alias) hang or fail, or
`tenx-rltec.local` resolves to a `100.64.x` address (or not at all), specifically when the
Zscaler tunnel is up.

**Cause:** with the Zscaler tunnel (`zcctun0`) active, Avahi can advertise/resolve the
**tunnel** interface address instead of the `wlan0` LAN IP. Documented in
`infra-notes/verify-zscaler.sh` ("Zscaler tunnel up: avahi may publish the tunnel addr
instead of the LAN IP — prefer raw LAN IPs"). Recent resolver work
(`infra-notes/resolved-zscaler.conf`, negative-cache disable) may have reduced this, but
it remains a known intermittent failure mode.

**Confirm:**
```bash
getent hosts tenx-rltec.local     # expect 192.168.1.x; a 100.64.x answer = this bug
resolvectl status                 # check which link/resolver is answering
./infra-notes/verify-zscaler.sh   # checks avahi-daemon + nss-mdns + this exact gotcha
```

**Fixes, in order:**
1. Flush and retry: `sudo resolvectl flush-caches`.
2. **Fall back to the reserved LAN IP** — in `~/.ssh/config` set the `tenx-lan` alias's
   `HostName` to tenx's DHCP-reserved `192.168.1.x` instead of `tenx-rltec.local`. Because
   the alias uses `HostKeyAlias tenx-rltec`, the host-key check still passes; nothing else
   changes. (Set the reservation on the router; record the IP here: `RESERVED_IP = TBD`.)
3. Confirm Avahi is healthy: `systemctl is-active avahi-daemon` and that `nss-mdns` is on the
   `hosts:` line in `/etc/nsswitch.conf` (both checked by `verify-zscaler.sh`).

> Why we still default to mDNS despite this: it needs zero client setup and the fallback is
> a one-line, host-key-safe swap. See [ADR-0009](adr/0009-ssh-transport-aliases-mdns-hostkey-pinned.md).

## `data-lan` unreachable but the address is correct → tenx is asleep

**Symptom:** `tenx-rltec.local`/the reserved IP resolves fine, but ping/SSH get "no route to
host" or time out.

**Cause:** `tenx-rltec` suspended/slept (this happened during setup — the box was powered on
but not logged in, then idled out and dropped off the LAN). No addressing change fixes a
sleeping host.

**Fixes:**
- On `tenx-rltec`, disable/extend suspend so it stays reachable for end-of-day pushes
  (Omarchy: check Hyprland idle/`hypridle` and systemd `sleep.conf` / `logind` settings).
- Or enable Wake-on-LAN and wake it before pushing.
- Verify it's actually up: `ping -c1 <addr>` then `ssh tenx-lan true`.

## Host-key verification failed on push

**Symptom:** SSH aborts with a host-key mismatch.

**Cause (good):** `StrictHostKeyChecking yes` + `HostKeyAlias tenx-rltec` is doing its job —
the host answering isn't tenx (e.g. a foreign `192.168.1.x` device, or tenx was reinstalled).

**Fix:** confirm you're really talking to tenx, then update the pinned key in `known_hosts`
only if the change is legitimate (e.g. tenx's OS was reinstalled).
