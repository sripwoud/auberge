# Headscale Self-Hosted Control Server — Implementation Plan

## Overview

Replace Tailscale SaaS with self-hosted Headscale so all coordination metadata stays on our infrastructure. Headscale is a single Go binary with SQLite, deployed as a systemd service behind Caddy. Embedded DERP relay for full sovereignty.

**Key decisions:**

- Auth: Pre-auth keys (same Ansible flow as today)
- DERP: Embedded in Headscale (no separate service)
- TLS: Caddy reverse proxy (consistent with existing stack)

---

## Architecture Difference from Other Services

Headscale is **publicly reachable** — unlike paperless/bichon which bind to Tailscale IPs. Tailscale clients need to reach Headscale _before_ they join the tailnet. This means:

- Caddy vhost binds to public interface (like yourls), NOT Tailscale IP
- DNS A record points to public VPS IP, NOT CGNAT IP
- UFW must allow STUN port 3478/UDP for embedded DERP
- The `tailscale` role must learn a new `--login-server` flag

---

## 1. New Files

### `ansible/roles/headscale/defaults/main.yml`

Key variables:

- `headscale_version` — pinned release tag (e.g. `"0.28.0"`)
- `headscale_port: 8080`
- `headscale_metrics_port: 9090`
- `headscale_stun_port: 3478`
- `headscale_sys_user: headscale`
- `headscale_sys_group: headscale`
- `headscale_config_dir: /etc/headscale`
- `headscale_data_dir: /var/lib/headscale`
- `headscale_db_path: /var/lib/headscale/db.sqlite`
- `headscale_subdomain: hs` (control plane endpoint: `hs.{{ primary_domain }}`)
- `headscale_domain: "{{ headscale_subdomain }}.{{ primary_domain }}"`
- `headscale_server_url: "https://{{ headscale_domain }}"`
- `headscale_ip_prefix_v4: "100.64.0.0/10"`
- `headscale_ip_prefix_v6: "fd7a:115c:a1e0::/48"`
- `headscale_magic_dns_enabled: true`
- `headscale_base_domain: "{{ primary_domain }}"`
- `headscale_derp_enabled: true`
- `headscale_derp_region_id: 999`
- `headscale_derp_region_name: "self-hosted"`
- `headscale_log_level: info`

### `ansible/roles/headscale/tasks/main.yml`

Deployment sequence (detailed in section 4).

### `ansible/roles/headscale/handlers/main.yml`

Two handlers:

- `Restart headscale` — `systemd_service: name=headscale state=restarted daemon_reload=true`
- `Restart caddy` — `systemd_service: name=caddy state=restarted`

### `ansible/roles/headscale/templates/headscale-config.yaml.j2`

Headscale YAML config. Key sections:

```yaml
server_url: {{ headscale_server_url }}
listen_addr: 127.0.0.1:{{ headscale_port }}
metrics_listen_addr: 127.0.0.1:{{ headscale_metrics_port }}
noise:
  private_key_path: {{ headscale_data_dir }}/noise_private.key
prefixes:
  v4: {{ headscale_ip_prefix_v4 }}
  v6: {{ headscale_ip_prefix_v6 }}
  allocation: sequential
derp:
  server:
    enabled: {{ headscale_derp_enabled | lower }}
    region_id: {{ headscale_derp_region_id }}
    region_code: self
    region_name: {{ headscale_derp_region_name }}
    stun_listen_addr: 0.0.0.0:{{ headscale_stun_port }}
  urls: []
  auto_update_enabled: false
database:
  type: sqlite
  sqlite:
    path: {{ headscale_db_path }}
    write_ahead_log: true
dns:
  magic_dns: {{ headscale_magic_dns_enabled | lower }}
  base_domain: {{ headscale_base_domain }}
  nameservers:
    global:
      - 1.1.1.1
      - 1.0.0.1
log:
  level: {{ headscale_log_level }}
logtail:
  enabled: false
policy:
  mode: file
  path: ""
```

### `ansible/roles/headscale/templates/headscale.service.j2`

Systemd unit. Binds to `127.0.0.1:{{ headscale_port }}` (Caddy fronts it). Full hardening block.

```ini
[Unit]
Description=Headscale - Tailscale control server
After=network-online.target
Wants=network-online.target

[Service]
User={{ headscale_sys_user }}
Group={{ headscale_sys_group }}
ExecStart=/usr/local/bin/headscale serve
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5

NoNewPrivileges=true
PrivateDevices=true
PrivateTmp=true
ProtectSystem=strict
ProtectControlGroups=true
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ReadWritePaths={{ headscale_data_dir }}
ReadOnlyPaths={{ headscale_config_dir }}
UMask=0027

[Install]
WantedBy=multi-user.target
```

### `ansible/roles/headscale/templates/headscale.caddyfile.j2`

Public-facing vhost (no `bind` directive = binds to all interfaces, like yourls):

```
{{ headscale_domain }} {
	reverse_proxy 127.0.0.1:{{ headscale_port }}
}
```

No `bind {{ tailscale_ip }}` — this must be publicly reachable.

---

## 2. Modified Files

### `ansible/roles/tailscale/defaults/main.yml`

Add:

```yaml
tailscale_login_server: ""
```

Empty string = use Tailscale SaaS (backwards compatible). When set, `tailscale up` uses `--login-server`.

### `ansible/roles/tailscale/tasks/main.yml`

Modify the `tailscale up` command to include `--login-server` when `tailscale_login_server` is set:

```yaml
- name: Connect to Tailscale network
  ansible.builtin.command: >-
    tailscale up
    --hostname={{ tailscale_hostname }}
    {% if tailscale_login_server | length > 0 %}--login-server={{ tailscale_login_server }}{% endif %}
    ...existing flags...
```

### `src/user_config.rs`

Add to TEMPLATE between `tailscale_api_key` and `tailscale_authkey` (alphabetical):

```toml
headscale_subdomain = ""
```

And add after existing `tailscale_authkey`:

```toml
tailscale_login_server = ""
```

Wait — `tailscale_login_server` sorts after `tailscale_authkey`. Current order:

```
tailscale_api_key = ""
tailscale_authkey = ""
```

New order:

```
headscale_subdomain = ""

...

tailscale_api_key = ""
tailscale_authkey = ""
tailscale_login_server = ""
```

`headscale_subdomain` goes between `freshrss_subdomain` and `navidrome_subdomain`.

### `src/services/dns.rs`

Add `"headscale_subdomain"` to `KNOWN_SUBDOMAIN_KEYS` (alphabetical, between `freshrss_subdomain` and `navidrome_subdomain`).

### `ansible/playbooks/infrastructure.yml`

Add headscale role BEFORE tailscale (headscale must be running before tailscale connects to it):

```yaml
- role: headscale
  tags: [infrastructure, network, vpn, headscale]
- role: tailscale
  tags: [infrastructure, network, vpn, tailscale]
```

### `ansible/roles/ufw/tasks/main.yml`

Add STUN port rule for embedded DERP:

```yaml
- name: Allow STUN traffic for Headscale DERP relay (port 3478)
  community.general.ufw:
    rule: allow
    port: 3478
    proto: udp
    comment: "Headscale STUN"
```

### `src/commands/backup.rs`

Add `headscale()` constructor to `AppBackupConfig`:

```rust
fn headscale() -> Self {
    Self {
        name: "headscale",
        systemd_services: vec!["headscale"],
        paths: vec!["/var/lib/headscale"],
        owner: Some(("headscale", "headscale")),
        db: None,
    }
}
```

Wire into `all()`, `by_name()`, and `--apps` help text.

---

## 3. Config Keys in `config.toml`

| Key                      | Required | Sensitive | Purpose                                         |
| ------------------------ | -------- | --------- | ----------------------------------------------- |
| `headscale_subdomain`    | yes      | no        | Subdomain for control plane (e.g. `hs`)         |
| `tailscale_login_server` | no       | no        | Headscale URL for `tailscale up --login-server` |

No sensitive keys needed — pre-auth keys are generated on Headscale CLI and injected into `tailscale_authkey` (existing key).

---

## 4. Deployment Sequence — `tasks/main.yml`

1. **Create system group** — `headscale`, `system: true`
2. **Create system user** — `headscale`, shell `/usr/sbin/nologin`, home `{{ headscale_data_dir }}`
3. **Create directories** — `{{ headscale_config_dir }}`, `{{ headscale_data_dir }}` with correct ownership
4. **Check installed version** — `stat` + `slurp` version file
5. **Download Headscale binary** (conditional) — from GitHub releases to `/usr/local/bin/headscale`, `mode: 0755`
6. **Deploy config** — template `headscale-config.yaml.j2` to `{{ headscale_config_dir }}/config.yaml`, `mode: 0640`, `owner: headscale`. Notify: `Restart headscale`
7. **Deploy systemd service** — template to `/etc/systemd/system/headscale.service`. Notify: `Restart headscale`
8. **Deploy Caddy vhost** — template to `/etc/caddy/sites/{{ headscale_domain }}.caddyfile`. Notify: `Restart caddy`
9. **Enable and start service** — `systemd_service`, `daemon_reload: true`
10. **Write installed version** (conditional)
11. **Display deployment summary**

---

## 5. Security Considerations

### Network architecture

- Headscale listens on `127.0.0.1:8080` — not directly exposed
- Caddy terminates TLS on public interface (Let's Encrypt via HTTP-01 or DNS-01)
- Embedded DERP STUN binds to `0.0.0.0:3478/UDP` — required for NAT traversal
- UFW allows 3478/UDP (STUN only, not full relay port)

### Systemd hardening

Same pattern as other services: `NoNewPrivileges`, `PrivateDevices`, `ProtectSystem=strict`, `ReadWritePaths` limited to data dir.

### Pre-auth key management

Pre-auth keys are generated via `headscale preauthkeys create` on the server. The key is placed in `config.toml` as `tailscale_authkey` (same field, different source). This is a manual step after initial Headscale deployment.

---

## 6. Migration Runbook (Post-Deployment)

After Headscale is deployed, migrating from Tailscale SaaS:

1. Deploy headscale role → service is running at `https://hs.yourdomain.com`
2. Create a Headscale user: `headscale users create default`
3. Generate pre-auth key: `headscale preauthkeys create --user default --reusable --expiration 24h`
4. Update `config.toml`:
   - `tailscale_authkey = "<new headscale pre-auth key>"`
   - `tailscale_login_server = "https://hs.yourdomain.com"`
5. On each node: `tailscale logout` then re-run `auberge ansible run --tags tailscale`
6. Verify: `tailscale status` shows all nodes connected via Headscale

This is a one-time migration. After this, the flow is identical to before.

---

## 7. Backup Integration

`AppBackupConfig::headscale()`:

- `systemd_services: vec!["headscale"]` — stop before backup
- `paths: vec!["/var/lib/headscale"]` — captures SQLite DB, noise keys, and DERP keys
- `owner: Some(("headscale", "headscale"))`
- `db: None` — SQLite is file-based, captured by rsync

---

## 8. Commit Plan

### Commit 1: `feat(headscale): add ansible role with config, systemd, and caddy templates`

**Files created:**

- `ansible/roles/headscale/defaults/main.yml`
- `ansible/roles/headscale/tasks/main.yml`
- `ansible/roles/headscale/handlers/main.yml`
- `ansible/roles/headscale/templates/headscale-config.yaml.j2`
- `ansible/roles/headscale/templates/headscale.service.j2`
- `ansible/roles/headscale/templates/headscale.caddyfile.j2`

Self-contained role, runnable but not wired.

### Commit 2: `feat(headscale): add UFW STUN rule for embedded DERP relay`

**Files modified:**

- `ansible/roles/ufw/tasks/main.yml` — add 3478/UDP rule

### Commit 3: `feat(tailscale): support custom login server for Headscale`

**Files modified:**

- `ansible/roles/tailscale/defaults/main.yml` — add `tailscale_login_server`
- `ansible/roles/tailscale/tasks/main.yml` — add `--login-server` flag

Backwards compatible: empty string = Tailscale SaaS.

### Commit 4: `feat(headscale): register subdomain in dns discovery and config template`

**Files modified:**

- `src/user_config.rs` — add `headscale_subdomain`, `tailscale_login_server` to TEMPLATE
- `src/services/dns.rs` — add `"headscale_subdomain"` to `KNOWN_SUBDOMAIN_KEYS`

### Commit 5: `feat(headscale): add to infrastructure playbook`

**Files modified:**

- `ansible/playbooks/infrastructure.yml` — add headscale role before tailscale

### Commit 6: `feat(headscale): add backup and restore support`

**Files modified:**

- `src/commands/backup.rs` — add `headscale()` constructor, wire into `all()` and `by_name()`

### Commit 7: `docs(headscale): add deployment and migration documentation`

**Files created/modified:**

- `docs/applications/networking/headscale.md`
- Update `docs/_sidebar.md`, `docs/applications/overview.md`
- Update `docs/configuration/environment-variables.md`

---

## Post-Implementation Checklist

- PR review of all changes as if reviewing another engineer's work
- Decide on recommendations and apply; iterate until tests pass
- Remove any unnecessary comments from all modified files
