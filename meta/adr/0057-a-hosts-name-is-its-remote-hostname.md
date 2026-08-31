# ADR-0057: A Host's name is its remote hostname

## Status

Accepted, 2026-08-31. Decided in #752; surfaced preparing #739's virgin run.

## Decision

**The name a Host carries in `hosts.toml` is the hostname its machine gets. Bootstrap consumes `inventory_hostname`; the `hostname` config key is deleted.**

Two mechanisms already treated the Host's name as the remote hostname: `ssh_identities_dir` derives the per-Host key directory from `inventory_hostname` (`group_vars/all/main.yml`), and `auberge host rename` runs `hostnamectl set-hostname <new-host-name>` then rewrites `/etc/hosts` with the same name (#520). Only `bootstrap.yml` disagreed, reading a fleet-wide `hostname` key — one value, so bootstrapping any second Host renamed it to the first Host's name, and `tailscale_hostname` (defaulting to `ansible_facts['hostname']`) carried the wrong name into the tailnet.

Two consequences:

- **One name for one fact.** `keys.yml` and `bootstrap.meta.yml` drop the key; `config init` stops offering it. A stale `hostname` left in `config.toml` is inert — nothing reads it.
- **`auberge headscale` loses its default-host shortcut.** `resolve_headscale_host` used `config.get("hostname")` as "the Host to SSH into", conflating the bootstrap rename value with the control-plane location. With the key gone it falls back to the host picker; #753 replaces the guess with a real answer (the Host whose config answers the headscale gate).

## Why

A fleet-wide key can only be right for a fleet of one. The defect was invisible for as long as the fleet had one Host per bootstrap era, and it is exactly the class of single-host assumption the ruche onboarding (#739) exists to flush out. Deriving from `inventory_hostname` removes the key rather than scoping it: the Host's name is already the identity every other mechanism keys on, so a second name for it could only ever agree or be a bug.

## Alternatives considered

- **Make `hostname` a per-Host override (#753 mechanism).** Rejected: there is no legitimate divergence to express — a Host whose hostname differs from its `hosts.toml` name breaks `host rename`'s rerun recovery (ADR-0024) and the identities layout. A key that must always equal another value is not configuration.
- **Keep the key, validate it equals the Host name.** Rejected: validation that only ever accepts one value is the derivation, plus a failure mode.
