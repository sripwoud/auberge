# Hosts Configuration

Hosts can be managed in two ways depending on your use case.

## XDG Config (Recommended for end users)

For users installing via `cargo install`, hosts are managed in `~/.config/auberge/hosts.toml`:

```bash
# Add a host interactively
auberge host add my-vps

# Or non-interactively
auberge host add my-vps 203.0.113.10 --user admin --port 22

# List hosts
auberge host list

# Show host details
auberge host show my-vps

# Edit host
auberge host edit my-vps

# Remove host
auberge host remove my-vps
```

The `hosts.toml` format:

```toml
[[hosts]]
name = "auberge"
address = "203.0.113.10"
user = "sripwoud"
port = 22
tags = ["production"]
description = "Main VPS"
ssh_key = "~/.ssh/identities/auberge/sripwoud"
tailscale_ip = "100.99.62.26"  # optional, see below
tailnet_tag = "data"           # optional, see below
```

### Optional fields

- `tailscale_ip` — cached Tailscale CGNAT IPv4 of the host. Populated by [`auberge host detect-tailscale-ip <name>`](cli-reference/host/detect-tailscale-ip.md) and consumed by `auberge dns set-all` to auto-fill DNS records for tailnet-only apps without per-app overrides.
- `tailnet_tag` — the host's tailnet trust tier: `trusted`, `data`, `agent`, or `standby`. One of exactly those four; anything else fails to parse, naming the legal values. Set by `auberge host add --tailnet-tag` or `auberge host edit` and shown as the `TIER` column in `auberge host list`. Never reaches Ansible; its consumer is the [per-run pre-auth mint](applications/networking/headscale.md), which stamps it on the key it mints so a host lands in its tier at enrollment.

> [!IMPORTANT]
> `tags` and `tailnet_tag` are different axes and must stay that way. `tags`
> become Ansible inventory groups (`all.children.<tag>`), read by
> `when: "'hermes' in group_names"` guards — they decide **which roles run**.
> `tailnet_tag` is the host's ACL trust tier — it decides **what the host may
> reach on the tailnet**. Deriving one from the other would mean adding an
> Ansible group silently moved a host's network trust. See ADR-0062.

The legal tiers are the ones the **shipped** ACL policy declares under
`tagOwners` (`ansible/roles/headscale/files/policy.hujson`), not the ones the
running server currently has loaded — a test holds the file and the CLI's type
equal, so a tier the CLI accepts is one the policy names. Whether headscale
_accepts_ it is a separate, later question, answered when the tag is applied:
its `TagExists` gate reads the loaded policy and refuses everything while none
is loaded, so it cannot validate a declaration. See ADR-0062.

## Ansible Inventory (Recommended for developers)

For development, keep using `ansible/inventory.yml` in the repository:

```yaml
all:
  children:
    vps:
      hosts:
        auberge:
          ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
          ansible_port: 22
          bootstrap_user: root
```

## Priority Order

The CLI checks hosts in this order:

1. `~/.config/auberge/hosts.toml` (if exists and not empty)
2. `ansible/inventory.yml` (fallback for developers)
3. Environment variables (legacy support)

## hosts.toml vs inventory.yml

- **hosts.toml**: User-specific hosts (backup operations)
  - Location: `~/.config/auberge/hosts.toml`
  - Not version controlled
  - Managed via `auberge host` commands

- **inventory.yml**: Ansible playbooks (shared infrastructure)
  - Location: `ansible/inventory.yml`
  - Version controlled
  - Used by `auberge ansible` commands
