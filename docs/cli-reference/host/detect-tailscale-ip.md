# auberge host detect-tailscale-ip

Detect and cache the host's Tailscale IPv4 address.

## Usage

```bash
auberge host detect-tailscale-ip [NAME]
# Alias: auberge h dti
```

If `NAME` is omitted, you'll be prompted to select a host.

## What it does

1. SSHs into the host using its existing SSH key.
2. Runs `tailscale ip -4` on the remote.
3. Picks the first CGNAT IPv4 address (`100.64.0.0/10`).
4. Persists it to `~/.config/auberge/hosts.toml` under the host's `tailscale_ip` field.

## Why

Once cached, `auberge dns set-all --host <name>` automatically points tailnet-only subdomains (apps with `tailnet_only: true` in their playbook meta — currently `bichon` and `paperless`) at this IP, without needing per-app `<app>_tailscale_ip` keys in `config.toml`.

## Examples

```bash
# Detect for a specific host
auberge host detect-tailscale-ip auberge

# Interactive (prompts for host)
auberge host detect-tailscale-ip
```

After running, `hosts.toml` gains a line like:

```toml
[[hosts]]
name = "auberge"
address = "203.0.113.10"
# …
tailscale_ip = "100.99.62.26"
```

## Failure modes

- **SSH key not found**: run `auberge ssh keygen --host <name>` first.
- **`tailscale` not on the remote `PATH`**: install Tailscale on the host (Auberge's `infrastructure` role does this).
- **No CGNAT IPv4 returned**: the Tailscale daemon may not be `Running`; check `systemctl status tailscaled` on the host.

## Related

- [auberge dns set-all](../dns/set-all.md) — consumer of `tailscale_ip`
- [Tailnet-only Subdomains](../../cli-reference/dns/set-all.md#tailnet-only-apps)
- [Hosts Configuration](../../configuration/hosts.md)
