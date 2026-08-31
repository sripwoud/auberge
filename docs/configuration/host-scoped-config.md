# Host-scoped Config

`config.toml` is fleet-wide by default: every key applies to every host. The reserved `[hosts.<name>]` table scopes keys to one host — for that host's runs, its entries override the top level in preflight validation and in the variables passed to Ansible.

```toml
domain = "example.com"
headscale_subdomain = "hs"
blocky_subdomain = "dns"

[hosts.agent-box]
headscale_subdomain = ""       # this host does not serve headscale
blocky_subdomain = ""          # nor blocky
tailscale_authkey = "!pa show tailnet/agent-box"
```

`<name>` matches the host's `hosts.toml` entry. Tables for names no host carries are inert.

## Blank overrides withdraw a role

`infrastructure.yml` gates `headscale` and `blocky` on their `*_subdomain` keys. A blank override makes the gate read unset for that host:

| Run against `agent-box`      | Result                                             |
| ---------------------------- | -------------------------------------------------- |
| untagged `infrastructure`    | headscale and blocky skipped, everything else runs |
| `-t headscale` / `-t blocky` | preflight fails naming the blanked key             |

Naming a gated role's tag asserts it runs, so asking for it on a host that withdrew the key is an error, not a silent no-op.

## What stays top-level

An `<app>_subdomain` is the app's record name in your one DNS zone — fleet identity, read by DNS discovery from the top level. Scope it per host only to withdraw the role (blank); give divergent _values_ per host only for keys that are genuinely per-host (`tailscale_authkey`, ports).

## Editing

`auberge config set` writes flat top-level keys and rejects dotted names. Edit host tables in the file: `auberge config edit`.
