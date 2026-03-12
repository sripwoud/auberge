# Bichon Email Archiving Ansible Role — Implementation Plan

## Overview

Add a `bichon` Ansible role following the Tailscale-only private service pattern (same as `paperless`). Bichon is a Rust binary downloaded from GitHub releases, exposed only via Tailscale + Caddy with DNS-01 TLS. Single systemd service, no Docker, no database.

---

## 1. New Files

### `ansible/roles/bichon/defaults/main.yml`

Holds all role defaults. No secrets here — those come from `config.toml` via Ansible extra vars.

Key variables:

- `bichon_version` — pinned release tag (e.g. `"0.3.7"`)
- `bichon_port: 15630`
- `bichon_bind_ip: 127.0.0.1`
- `bichon_sys_user: bichon`
- `bichon_sys_group: bichon`
- `bichon_install_dir: /opt/bichon`
- `bichon_root_dir: /opt/bichon/data`
- `bichon_index_dir: /opt/bichon/data/index`
- `bichon_data_dir: /opt/bichon/data/store`
- `bichon_log_level: info`
- `bichon_subdomain: bichon`
- `bichon_domain: "{{ bichon_subdomain }}.{{ domain }}"`
- `bichon_public_url: "https://{{ bichon_domain }}"`
- `bichon_encrypt_password_file: "{{ bichon_install_dir }}/encrypt.password"`
- `bichon_release_url` — constructed from `bichon_version` pointing to the GitHub releases asset URL

### `ansible/roles/bichon/tasks/main.yml`

Deployment sequence (detailed in section 4).

### `ansible/roles/bichon/handlers/main.yml`

Two handlers:

- `Restart bichon` — `systemd_service: name=bichon state=restarted daemon_reload=true`
- `Restart caddy` — `systemd_service: name=caddy state=restarted`

### `ansible/roles/bichon/templates/bichon.service.j2`

Systemd unit for the bichon binary. Binds to `127.0.0.1:{{ bichon_port }}`. Uses `EnvironmentFile` pointing to the deployed env file. Full systemd hardening block matching the paperless pattern.

### `ansible/roles/bichon/templates/bichon.caddyfile.j2`

Single vhost block mirroring the paperless pattern:

```
{{ bichon_domain }} {
  bind {{ bichon_tailscale_ip }}
  tls {
    dns cloudflare {env.CLOUDFLARE_DNS_API_TOKEN}
  }
  reverse_proxy 127.0.0.1:{{ bichon_port }}
}
```

### `ansible/roles/bichon/templates/bichon.env.j2`

Environment file (`mode: 0600`, owned by `bichon:bichon`):

```
BICHON_ROOT_DIR={{ bichon_root_dir }}
BICHON_INDEX_DIR={{ bichon_index_dir }}
BICHON_DATA_DIR={{ bichon_data_dir }}
BICHON_LOG_LEVEL={{ bichon_log_level }}
BICHON_BIND_IP={{ bichon_bind_ip }}
BICHON_HTTP_PORT={{ bichon_port }}
BICHON_PUBLIC_URL={{ bichon_public_url }}
BICHON_ENCRYPT_PASSWORD_FILE={{ bichon_encrypt_password_file }}
```

---

## 2. Modified Files

### `src/user_config.rs`

Add to the `TEMPLATE` constant in alphabetical position (between `baikal_*` and `blocky_*`):

```toml
bichon_encryption_password = ""
bichon_subdomain = ""
bichon_tailscale_ip = ""
```

`bichon_encryption_password` is the config key used to supply the password value; it ends in `_password` so it is automatically redacted by `SENSITIVE_SUFFIXES`. At deploy time the role writes this value to a file and passes the path via `BICHON_ENCRYPT_PASSWORD_FILE`. `bichon_tailscale_ip` is an optional override matching the existing `paperless_tailscale_ip` pattern.

### `src/services/dns.rs`

Add `"bichon_subdomain"` to `KNOWN_SUBDOMAIN_KEYS`. This makes `auberge dns` aware of the subdomain and enables the `bichon_tailscale_ip` override lookup via the existing `discover_subdomains()` logic.

### `ansible/playbooks/apps.yml`

Add entry in alphabetical order (between `baikal` and `blocky`):

```yaml
- role: bichon
  tags: [apps, storage, email, bichon]
```

### `src/commands/backup.rs`

Four targeted changes:

1. Add `bichon` to the `--apps` help text strings in `BackupCommands::Create` and `BackupCommands::Restore`.
2. Add `Self::bichon()` to `AppBackupConfig::all()`.
3. Add `"bichon" => Some(Self::bichon())` arm to `AppBackupConfig::by_name()`.
4. Add the constructor:

```rust
fn bichon() -> Self {
    Self {
        name: "bichon",
        systemd_services: vec!["bichon"],
        paths: vec!["/opt/bichon/data"],
        owner: Some(("bichon", "bichon")),
        db: None,
    }
}
```

---

## 3. Config Keys in `config.toml`

| Key                          | Required | Sensitive | Purpose                                                                                  |
| ---------------------------- | -------- | --------- | ---------------------------------------------------------------------------------------- |
| `bichon_subdomain`           | yes      | no        | Subdomain for Caddy vhost and DNS A record                                               |
| `bichon_encryption_password` | yes      | yes       | Written to `bichon_encrypt_password_file`; path passed as `BICHON_ENCRYPT_PASSWORD_FILE` |
| `bichon_tailscale_ip`        | no       | no        | Override Tailscale IP (auto-discovered if empty)                                         |

No `bichon_admin_password` is required — the service uses access-token auth (always enabled; `BICHON_ENABLE_ACCESS_TOKEN` is deprecated since v0.2.0). The encryption password is the only secret required at deploy time. It is permanent — changing it after initial deployment requires a complete data wipe.

---

## 4. Deployment Sequence — `tasks/main.yml`

All steps inside a `block:` for clean failure grouping, matching the paperless structure:

1. **Validate required credentials** — `ansible.builtin.assert` that `bichon_encryption_password` is defined and non-empty. Fail fast before any filesystem changes. (The role then writes this value to `bichon_encrypt_password_file` and passes the path via `BICHON_ENCRYPT_PASSWORD_FILE`.)

2. **Get Tailscale status** — `tailscale status --json`, `register: bichon_tailscale_status_raw`, `failed_when: false`.

3. **Parse Tailscale status** — `set_fact` from stdout JSON, conditional on `rc == 0`.

4. **Set Tailscale IPv4 fact** — filter `Self.TailscaleIPs` selecting the IPv4 CGNAT address (`^[0-9]+\.` match), identical logic to paperless.

5. **Fail if Tailscale not available** — hard fail when `bichon_tailscale_ip` is not defined. Rationale: bichon is private-only, Tailscale is non-negotiable.

6. **Display Tailscale IP** — debug message showing the resolved IP.

7. **Create system group** — `bichon`, `system: true`.

8. **Create system user** — `bichon`, shell `/usr/sbin/nologin`, home `{{ bichon_install_dir }}`, `create_home: false`, `system: true`.

9. **Create directories** — `bichon_install_dir`, `bichon_root_dir`, `bichon_index_dir`, `bichon_data_dir` — all `mode: 0750`, owned `bichon:bichon`.

10. **Check installed version** — `stat: path={{ bichon_install_dir }}/version`.

11. **Read installed version** — `slurp` the version file when it exists.

12. **Set installed version fact** — b64decode + trim if file exists, else `''`.

13. **Download binary** (conditional: `when: bichon_installed_version != bichon_version`) — `get_url` the release asset to `/tmp/bichon`, `mode: 0755`.

14. **Install binary** — copy `/tmp/bichon` to `{{ bichon_install_dir }}/bichon`, `owner: root`, `group: root`, `mode: 0755`. Remove `/tmp/bichon` afterward.

15. **Deploy environment file** — template `bichon.env.j2` to `{{ bichon_install_dir }}/bichon.env`, `mode: 0600`, `owner: bichon`, `group: bichon`, `no_log: true`. Notify: `Restart bichon`.

16. **Validate Cloudflare token** — `assert` that `cloudflare_dns_api_token` is defined and non-empty (mirrors paperless).

17. **Deploy Caddy site config** — template `bichon.caddyfile.j2` to `/etc/caddy/sites/{{ bichon_domain }}.caddyfile`, `mode: 0644`. Notify: `Restart caddy`.

18. **Deploy systemd service** — template `bichon.service.j2` to `/etc/systemd/system/bichon.service`, `mode: 0644`. Notify: `Restart bichon`.

19. **Enable and start service** — `systemd_service`, `daemon_reload: true`, `enabled: true`, `state: started`.

20. **Write installed version** (conditional: `when: bichon_installed_version != bichon_version`) — copy `bichon_version` string to `{{ bichon_install_dir }}/version`.

21. **Display deployment summary** — debug msg block with domain, bind address, data dir, service status command.

---

## 5. Security Considerations

### Systemd hardening (in `bichon.service.j2`)

```ini
After=network.target tailscaled.service
Requires=tailscaled.service

NoNewPrivileges=true
PrivateDevices=true
PrivateTmp=true
ProtectSystem=strict
ProtectControlGroups=true
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ReadWritePaths={{ bichon_root_dir }}
UMask=0027
```

### Network isolation

- Binary binds to `127.0.0.1:15630` only — not reachable from LAN or public internet at the process level.
- Caddy reverse proxy binds exclusively to the Tailscale IP (CGNAT `100.64.0.0/10` range).
- DNS A record points to the CGNAT IP, unreachable without Tailscale membership.
- No UFW rule changes needed; existing `ufw` role already manages Tailscale interface.

### Encryption at rest

- Encryption covers credentials and metadata only — email content is not encrypted.
- The password is delivered via a dedicated file (`bichon_encrypt_password_file`, `mode: 0600`, owned by `bichon:bichon`). Its path is passed to the process as `BICHON_ENCRYPT_PASSWORD_FILE`.
- The file-write task uses `no_log: true` to prevent the password appearing in Ansible run output.
- The `bichon_encryption_password` config key is automatically redacted in `auberge config list` due to the `_password` suffix in `SENSITIVE_SUFFIXES`.
- The encryption password is permanent. Changing it after initial deployment requires a complete data wipe.

---

## 6. Backup Integration

`AppBackupConfig::bichon()`:

- `systemd_services: vec!["bichon"]` — service is stopped before rsync, restarted after (existing backup orchestration handles this).
- `paths: vec!["/opt/bichon/data"]` — captures `bichon_root_dir` which contains both `index` and `store` subdirs in a single rsync pass.
- `owner: Some(("bichon", "bichon"))` — rsync sets correct ownership on restore.
- `db: None` — no external database; bichon is fully self-contained on disk.

Included in `AppBackupConfig::all()` so `auberge backup create` without `--apps` captures it by default.

---

## 7. Commit Plan

### Commit 1: `feat(bichon): add ansible role defaults, templates, and tasks`

**Files created:**

- `ansible/roles/bichon/defaults/main.yml`
- `ansible/roles/bichon/tasks/main.yml`
- `ansible/roles/bichon/handlers/main.yml`
- `ansible/roles/bichon/templates/bichon.service.j2`
- `ansible/roles/bichon/templates/bichon.caddyfile.j2`
- `ansible/roles/bichon/templates/bichon.env.j2`

Self-contained: role is fully runnable but not yet wired into any playbook or CLI.

---

### Commit 2: `feat(bichon): register subdomain in dns discovery and config template`

**Files modified:**

- `src/user_config.rs` — add `bichon_encryption_password`, `bichon_subdomain`, `bichon_tailscale_ip` to TEMPLATE (password value is written to file at deploy time; path passed as `BICHON_ENCRYPT_PASSWORD_FILE`)
- `src/services/dns.rs` — add `"bichon_subdomain"` to `KNOWN_SUBDOMAIN_KEYS`

After this commit: `auberge dns` discovers the bichon subdomain; `auberge config list` shows/redacts bichon keys correctly.

---

### Commit 3: `feat(bichon): add to apps playbook`

**Files modified:**

- `ansible/playbooks/apps.yml` — add bichon role with tags

After this commit: `auberge ansible run --tags bichon` deploys the service end-to-end.

---

### Commit 4: `feat(bichon): add backup and restore support`

**Files modified:**

- `src/commands/backup.rs` — add `bichon()` constructor, wire into `all()` and `by_name()`, update `--apps` help text strings

After this commit: `auberge backup create` and `auberge backup restore` handle bichon data.

---

## Post-Implementation Checklist

- PR review of all changes as if reviewing another engineer's work.
- Decide on which recommendations to apply; iterate until tests pass.
- Remove any unnecessary comments from all modified files.
