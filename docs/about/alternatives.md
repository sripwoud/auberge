# Comparison with Self-Hosting Platforms

How Auberge compares to established self-hosting platforms — and why it exists.

## Context

Several platforms make self-hosting accessible: web UIs, one-click installs, app stores. They solve real problems for real users. Auberge takes a different approach — a Rust CLI that drives Ansible playbooks to deploy native systemd services on a minimal VPS. No containers, no web UI, no app store.

This page explains where these platforms excel, where their priorities diverge from Auberge's, and what motivated building something new.

## YunoHost

[YunoHost](https://yunohost.org/) is a Debian-based distribution with a web UI, LDAP-based SSO, one-click app installs, and integrated backups. Maintained since 2012 by a French nonprofit, it's the most mature community-driven option.

**Strengths:**

- Web UI for everything — install/remove apps, manage users, configure domains
- SSO via LDAP — one account per user across all apps
- Integrated backup with BorgBackup support (encrypted, deduplicated, remote)
- Large app catalog (~500 packaged apps) with community-reviewed security config
- Only 2 CVEs in core project history

**Different trade-offs:**

- Installs Nginx, Postfix, Dovecot, Metronome (XMPP), LDAP, Dnsmasq, and SSOwat as base services — useful if you need them, overhead if you don't
- The base stack isn't swappable (can't replace Nginx with Caddy, can't remove the mail server)
- Expects to own the machine — running custom services alongside it (Headscale, custom DNS) means working around the abstraction
- Recovery means restoring from backup rather than re-running a playbook on a fresh box
- Some apps I use aren't packaged: Headscale, Blocky, Colporteur, Bichon, Booklore

YunoHost is an excellent choice for anyone who wants to self-host without thinking about infrastructure. It covers the most ground with the least effort.

## Umbrel

[Umbrel](https://umbrel.com/) (umbrelOS) is a Docker-based home server OS with a polished app-store UI, primarily focused on Bitcoin nodes and media servers.

**Strengths:**

- Beautiful, consumer-friendly UI
- Large containerized app catalog (~300 apps)
- Active community and ecosystem

**Different trade-offs:**

- Shifted to PolyForm Noncommercial "Source Available" license — can't be freely forked or redistributed
- No HTTPS by default — a concern for any VPS exposed to the internet
- Designed for dedicated hardware on a LAN, not internet-facing VPS deployments
- Docker-only architecture with a proprietary management layer

The licensing change is a dealbreaker for FOSS-principled setups, and the LAN-first design makes it a poor fit for VPS deployments.

## CasaOS

[CasaOS](https://casaos.io/) is a lightweight Docker management UI that installs on top of Debian/Ubuntu, developed by IceWhale Technology (ZimaBoard makers).

**Strengths:**

- Lightweight and easy to install on existing systems
- Clean UI for managing Docker containers
- Low barrier to entry for beginners

**Different trade-offs:**

- LAN-oriented design — the web UI has no HTTPS, and on a fresh VPS install the setup page is exposed on the public IP without authentication
- No built-in backup system, LDAP/SSO, or Let's Encrypt management
- IceWhale is shifting focus to ZimaOS (their commercial NAS OS), raising questions about CasaOS's long-term maintenance
- For someone comfortable with a CLI, it adds a UI layer but not much operational tooling

CasaOS works well as a home media server on a local network. For internet-facing VPS deployments, the security model needs additional hardening.

## ZimaOS

[ZimaOS](https://zimaos.com/) is CasaOS's successor — a full standalone NAS operating system from IceWhale.

**Different trade-offs:**

- x86-only, bare-metal only — meant to be flashed as the sole OS, primarily on Zima hardware
- NAS-focused features (RAID, ZFS/Btrfs, GPU passthrough) that are irrelevant to lightweight VPS deployments
- Overwrites customizations on update

Interesting for a home NAS appliance, but not designed for the VPS use case.

## Why Auberge exists

The platforms above solve real problems. Auberge targets a narrower set of priorities:

- **Native services**: systemd services with no container overhead on a 2-4GB VPS
- **Full control**: every service, every config file, every firewall rule
- **Reproducibility**: if the server dies, `auberge deploy --all` on a fresh box gets you back
- **Minimal attack surface**: only the services you chose, nothing else
- **Pick your tools**: Caddy over Nginx, Blocky over Dnsmasq, Headscale for mesh networking — opinionated defaults, but the Ansible roles are yours to modify
- **FOSS licensing**: AGPL-3.0, no commercial pivot risk

Auberge is selfware — software I built to manage my own infrastructure. My needs drive every decision. That said, if you share similar priorities — minimal footprint, reproducibility, direct control — you might find it useful too. Contributions, feedback, and ideas are welcome. The [Contributing guide](../development/contributing.md) is a good place to start.

The platforms above do more than I need in some places, less in others. Auberge is shaped around my specific priorities.

## Comparison at a glance

|                       | Auberge                 | YunoHost                 | Umbrel             | CasaOS             |
| --------------------- | ----------------------- | ------------------------ | ------------------ | ------------------ |
| **License**           | AGPL-3.0                | AGPL-3.0                 | PolyForm NC        | Apache 2.0         |
| **Architecture**      | Native systemd          | Native (Debian)          | Docker             | Docker             |
| **Web UI**            | No (CLI only)           | Yes                      | Yes                | Yes                |
| **VPS-suitable**      | Yes                     | Yes                      | No                 | Risky              |
| **Base RAM overhead** | ~0 (just your services) | ~300-500MB               | ~200-400MB         | ~100-200MB         |
| **Reproducible**      | Yes (Ansible)           | No (restore from backup) | No                 | No                 |
| **SSO/LDAP**          | No (per-app auth)       | Yes                      | No                 | No                 |
| **Backup**            | Built-in                | Built-in + Borg          | Manual             | Manual             |
| **App catalog**       | Curated                 | ~500 packaged            | ~300 containerized | ~100 containerized |
| **Customizable**      | Fully                   | Limited                  | Minimal            | Moderate           |
| **HTTPS**             | Caddy (auto)            | Let's Encrypt (auto)     | Not by default     | Not by default     |

## Related Pages

- [Architecture Decisions](architecture-decisions.md) - Why Auberge is built the way it is
- [Roadmap](roadmap.md) - Future direction
- [Contributing](../development/contributing.md) - How to get involved
