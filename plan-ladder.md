# Ladder HTTP Web Proxy Ansible Role — Implementation Plan

## Overview

Add a `ladder` Ansible role to deploy [everywall/ladder](https://github.com/everywall/ladder) — a Go-based HTTP web proxy used for testing/analyzing paywall implementations and content delivery. Public exposure with Caddy `basic_auth`, pinned upstream binary with SHA256 verification, hybrid ruleset source (upstream URL by default, optional local override), no FlareSolverr, no backup integration (stateless app).

Locked decisions (from planning conversation):

| Decision         | Resolution                                                                                           |
| ---------------- | ---------------------------------------------------------------------------------------------------- |
| Exposure & auth  | Public HTTPS, Caddy `basic_auth` (single source), `caddy hash-password`                              |
| Ruleset source   | Hybrid — `ladder_ruleset_url` default to upstream; optional `ladder_ruleset_src` local file override |
| Version strategy | Pinned `ladder_version` + SHA256 verification from upstream `checksums.txt`                          |
| FlareSolverr     | Skip role; plumb optional `ladder_flaresolverr_host` env var only                                    |
| Backup           | Skip — stateless (no DB, no user data, binary re-fetched on deploy)                                  |
| Internal port    | `8086` (loopback only, Caddy fronts it)                                                              |
| Subdomain        | `ladder_subdomain` (required user config, no default)                                                |
| Auth password    | `ladder_basic_auth_password` (auto-redacted by `_password` suffix)                                   |

---

## A) File Layout

### New files

| Path                                               | Purpose                                                                                                                                                            |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ansible/roles/ladder/defaults/main.yml`           | Role defaults: paths, version, port, ruleset URL, env-var defaults, `ladder_flaresolverr_host: ""`                                                                 |
| `ansible/roles/ladder/tasks/main.yml`              | Deploy sequence: user/group → dirs → binary download + checksum verify → optional local ruleset copy → caddy hash-password → systemd unit → Caddyfile → DNS record |
| `ansible/roles/ladder/handlers/main.yml`           | `Restart ladder`, `Restart caddy`                                                                                                                                  |
| `ansible/roles/ladder/templates/ladder.service.j2` | Systemd unit binding `127.0.0.1:{{ ladder_port }}`, hardening block, conditional `FLARESOLVERR_HOST`                                                               |
| `ansible/roles/ladder/templates/Caddyfile.j2`      | Public vhost: `basic_auth` + `reverse_proxy 127.0.0.1:{{ ladder_port }}`                                                                                           |
| `docs/applications/apps/ladder.md`                 | User-facing docs                                                                                                                                                   |
| `plan-ladder.md`                                   | This plan                                                                                                                                                          |

### Modified files

| Path                            | Change                                                                |
| ------------------------------- | --------------------------------------------------------------------- |
| `ansible/playbooks/apps.yml`    | Insert role entry alphabetically; tags `[apps, web, proxy, ladder]`   |
| `src/user_config.rs`            | Add `ladder_basic_auth_password` and `ladder_subdomain` to `TEMPLATE` |
| `config.example.toml`           | Mirror the two keys                                                   |
| `src/services/dns.rs`           | Add `"ladder_subdomain"` to `KNOWN_SUBDOMAIN_KEYS`                    |
| `src/commands/deploy.rs`        | Extend `test_get_app_names_returns_roles` with `ladder` assertion     |
| `README.md`                     | Add ladder to Apps list                                               |
| `docs/_sidebar.md`              | Add `[Ladder](applications/apps/ladder.md)` under Apps                |
| `docs/applications/overview.md` | Add ladder entry                                                      |

### Explicitly NOT touched

- `src/commands/backup.rs` — stateless app
- `CHANGELOG.md` — release-please-managed
- No `flaresolverr` role — gated for a future PR
- No local ruleset file in `files/` — only added if `ladder_ruleset_src` is set by the operator

---

## B) Block / Template Structure

### `tasks/main.yml`

Single top-level `block:` "Install and configure Ladder", composed of:

1. System group + user (`ladder`, system, nologin, home `/opt/ladder`)
2. Directories: `/opt/ladder` (`0755 ladder:ladder`), `/etc/ladder` (`0750 root:ladder`)
3. Version reconciliation: stat + slurp `/opt/ladder/version` → set `ladder_installed_version` fact
4. Binary install (gated by `ladder_installed_version != ladder_version`):
   - Fetch `checksums.txt` (delegate_to localhost)
   - Parse SHA256 for `ladder_linux_amd64.tar.gz`
   - `get_url` archive with `checksum: "sha256:{{ ... }}"`
   - `unarchive` into `/opt/ladder` → notify `Restart ladder`
   - Ensure `0755` on the binary; write version sentinel
5. Ruleset selection: if `ladder_ruleset_src` set → copy local file to `/etc/ladder/ruleset.yaml` (`0640 root:ladder`) + notify Restart; set `ladder_effective_ruleset` fact accordingly
6. `caddy hash-password --plaintext` → `ladder_hashed_password` fact (`no_log`, `changed_when: false`)
7. Template `ladder.service.j2` → `/etc/systemd/system/ladder.service` → notify `Restart ladder`
8. `systemd_service: name=ladder enabled=true state=started daemon_reload=true`
9. Template `Caddyfile.j2` → `/etc/caddy/sites/{{ ladder_domain }}.caddyfile` → notify `Restart caddy`
10. `include_role: name=dns_record` with standard arg shape

### `handlers/main.yml`

- `Restart ladder` — `systemd_service: name=ladder state=restarted daemon_reload=true`
- `Restart caddy` — `systemd_service: name=caddy state=restarted`

### `templates/ladder.service.j2`

Sections: `[Unit]` (network-online wants/after), `[Service]` (Type=simple, User/Group=ladder, Environment block per pseudocode + conditional `FLARESOLVERR_HOST`, ExecStart, hardening: NoNewPrivileges/PrivateDevices/PrivateTmp/ProtectSystem=strict/ProtectControlGroups/ProtectHome/ProtectKernelTunables/ProtectKernelModules/ReadOnlyPaths=/etc/ladder/UMask=0022/Restart=on-failure/RestartSec=5s), `[Install]` WantedBy=multi-user.target.

### `templates/Caddyfile.j2`

Single vhost `{{ ladder_domain }}` with `basic_auth { admin_user_name ladder_hashed_password }`, `reverse_proxy 127.0.0.1:{{ ladder_port }}` (with Host/X-Real-IP/X-Forwarded-For/X-Forwarded-Proto headers), `encode gzip`, log to `/var/log/caddy/{{ ladder_domain }}.log`.

### Rust edits — narrow & surgical

- `src/user_config.rs::TEMPLATE` — alphabetical insert of two keys
- `src/services/dns.rs::KNOWN_SUBDOMAIN_KEYS` — alphabetical insert
- `src/commands/deploy.rs::test_get_app_names_returns_roles` — extend assertion list

---

## C) Function Pseudocode

### `defaults/main.yml`

```yaml
ladder_install_path: /opt/ladder
ladder_config_dir: /etc/ladder
ladder_sys_user: ladder
ladder_sys_group: ladder
ladder_port: 8086
ladder_domain: "{{ ladder_subdomain }}.{{ domain }}"
ladder_version: "0.0.22"
ladder_binary_url: "https://github.com/everywall/ladder/releases/download/v{{ ladder_version }}/ladder_linux_amd64.tar.gz"
ladder_checksum_url: "https://github.com/everywall/ladder/releases/download/v{{ ladder_version }}/checksums.txt"
ladder_ruleset_url: "https://raw.githubusercontent.com/everywall/ladder-rules/main/ruleset.yaml"
# ladder_ruleset_src: ""           # opt-in local override
ladder_user_agent: "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)"
ladder_x_forwarded_for: "66.249.66.1"
ladder_log_urls: true
ladder_disable_form: false
ladder_expose_ruleset: false
ladder_allowed_domains: ""
ladder_allowed_domains_ruleset: false
ladder_flaresolverr_host: ""
```

### `tasks/main.yml` pseudocode

```
block: "Install and configure Ladder"
  - group ladder (system)
  - user ladder (system, nologin, home=/opt/ladder, no create_home)
  - dir /opt/ladder              owner=ladder:ladder mode=0755
  - dir /etc/ladder              owner=root:ladder  mode=0750

  - stat /opt/ladder/version           → version_file
  - if version_file.exists: slurp it   → ladder_installed_version_raw
  - set_fact ladder_installed_version  = decoded|trim or ''

  - when ladder_installed_version != ladder_version:
      - delegate_to: localhost
        uri GET {{ ladder_checksum_url }}    → ladder_checksum_content
      - set_fact ladder_checksum_sha256 =
          (lines | select match "ladder_linux_amd64.tar.gz" | first | split | first)
      - get_url {{ ladder_binary_url }}      → /tmp/ladder-{{ ver }}.tar.gz
            checksum: "sha256:{{ ladder_checksum_sha256 }}"
      - unarchive → /opt/ladder owner=ladder:ladder mode=0755
            notify: Restart ladder
      - file /opt/ladder/ladder owner=ladder:ladder mode=0755
      - copy content="{{ ladder_version }}" → /opt/ladder/version mode=0644

  - set_fact ladder_use_local_ruleset = (ladder_ruleset_src is defined and length>0)
  - if ladder_use_local_ruleset:
      - copy {{ ladder_ruleset_src }} → /etc/ladder/ruleset.yaml
            owner=root:ladder mode=0640
            notify: Restart ladder
      - set_fact ladder_effective_ruleset = "/etc/ladder/ruleset.yaml"
    else:
      - set_fact ladder_effective_ruleset = "{{ ladder_ruleset_url }}"

  - command caddy hash-password --plaintext "{{ ladder_basic_auth_password }}"
        register: ladder_hash_result   changed_when: false   no_log: true
  - set_fact ladder_hashed_password = ladder_hash_result.stdout | trim

  - template ladder.service.j2 → /etc/systemd/system/ladder.service mode=0644
        notify: Restart ladder
  - systemd_service ladder enabled=true state=started daemon_reload=true

  - template Caddyfile.j2 → /etc/caddy/sites/{{ ladder_domain }}.caddyfile mode=0644
        notify: Restart caddy

  - include_role dns_record
        vars: subdomain={{ ladder_subdomain }} domain={{ domain }}
              cloudflare_api_token={{ cloudflare_dns_api_token }}
```

### `templates/ladder.service.j2`

```
[Unit]
Description=Ladder HTTP web proxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={{ ladder_sys_user }}
Group={{ ladder_sys_group }}
Environment=PORT={{ ladder_port }}
Environment=USER_AGENT={{ ladder_user_agent }}
Environment=X_FORWARDED_FOR={{ ladder_x_forwarded_for }}
Environment=RULESET={{ ladder_effective_ruleset }}
Environment=LOG_URLS={{ ladder_log_urls | string | lower }}
Environment=DISABLE_FORM={{ ladder_disable_form | string | lower }}
Environment=EXPOSE_RULESET={{ ladder_expose_ruleset | string | lower }}
Environment=ALLOWED_DOMAINS={{ ladder_allowed_domains }}
Environment=ALLOWED_DOMAINS_RULESET={{ ladder_allowed_domains_ruleset | string | lower }}
{% if ladder_flaresolverr_host | length > 0 %}
Environment=FLARESOLVERR_HOST={{ ladder_flaresolverr_host }}
{% endif %}
ExecStart={{ ladder_install_path }}/ladder
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal
NoNewPrivileges=true
PrivateDevices=true
PrivateTmp=true
ProtectSystem=strict
ProtectControlGroups=true
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ReadOnlyPaths={{ ladder_config_dir }}
UMask=0022

[Install]
WantedBy=multi-user.target
```

### `templates/Caddyfile.j2`

```
{{ ladder_domain }} {
    basic_auth {
        {{ admin_user_name }} {{ ladder_hashed_password }}
    }

    reverse_proxy 127.0.0.1:{{ ladder_port }} {
        header_up Host {host}
        header_up X-Real-IP {remote_ip}
        header_up X-Forwarded-For {remote_ip}
        header_up X-Forwarded-Proto {scheme}
    }

    encode gzip

    log {
        output file /var/log/caddy/{{ ladder_domain }}.log
    }
}
```

### Rust edits

```rust
// src/user_config.rs — TEMPLATE constant, alphabetical insert
ladder_basic_auth_password = ""
ladder_subdomain = ""

// src/services/dns.rs — KNOWN_SUBDOMAIN_KEYS array, alphabetical insert
"ladder_subdomain",

// src/commands/deploy.rs — extend existing test
fn test_get_app_names_returns_roles() {
    let apps = get_app_names().unwrap();
    assert!(apps.contains(&"paperless".to_string()));
    assert!(apps.contains(&"baikal".to_string()));
    assert!(apps.contains(&"freshrss".to_string()));
    assert!(apps.contains(&"blocky".to_string()));
    assert!(apps.contains(&"ladder".to_string()));
    assert!(!apps.contains(&"caddy".to_string()));
}

// config.example.toml — alphabetical insert mirroring TEMPLATE
ladder_basic_auth_password = ""
ladder_subdomain = ""
```

### Failure cases

- Checksum mismatch → `get_url` fails the play (intended security gate)
- `ladder_basic_auth_password` empty → caught by `validate_config_for_deploy` required-keys check (verify during implementation; if not picked up, add an explicit `assert:` task at top of role)
- `ladder_ruleset_src` points at a non-existent local file → `copy` fails with a clear error
- Upstream binary URL/asset name change → single PR fix via `ladder_version` bump
- GitHub release fetch hits rate limit → out of scope (would add Authorization header)

---

## D) TDD Plan

### Layers in scope

- **Backend (Rust):** `user_config.rs` template, `dns.rs` subdomain registration, `deploy.rs` discovery test
- **Infra (Ansible):** new role + `apps.yml` registration
- **Docs (Markdown):** sidebar, overview, app page, README

No frontend, no middletier.

### Tooling (already in `mise.toml`)

- `mise run test` → `cargo nextest run`
- `mise run lint-rust` → `cargo clippy`
- `mise run lint-ansible` → `ansible-lint` in `ansible/`
- `mise run fmt` → `dprint fmt -c .dprint.jsonc`
- `hk` pre-commit hook runs `ansible`, `clippy`, `dprint` on staged files (no `--no-verify`)

### Unit tests (Rust)

| #  | Test                                          | Location                 | Asserts                                                                      |
| -- | --------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------- |
| U1 | `test_get_app_names_returns_roles` (extended) | `src/commands/deploy.rs` | `apps.contains("ladder")` plus existing assertions                           |
| U2 | `test_known_subdomain_keys_includes_ladder`   | `src/services/dns.rs`    | `KNOWN_SUBDOMAIN_KEYS.contains(&"ladder_subdomain")`                         |
| U3 | `test_template_includes_ladder_keys`          | `src/user_config.rs`     | `TEMPLATE` contains both `ladder_basic_auth_password` and `ladder_subdomain` |
| U4 | `_password` suffix redaction                  | `src/user_config.rs`     | Conditional — only added if no equivalent test exists today                  |

### Integration tests (Ansible — manual gate before PR)

| #   | Scenario                     | Command                                                                         | Pass criteria                               |
| --- | ---------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------- |
| I1  | Role lints                   | `cd ansible && ansible-lint roles/ladder`                                       | Zero warnings                               |
| I2  | Check-mode against host      | `auberge deploy ladder --check -H <host> -f`                                    | Exits 0; idempotent after first real deploy |
| I3  | Full deploy                  | `auberge deploy ladder -H <host> -f`                                            | Exits 0; `systemctl status ladder` active   |
| I4  | Re-deploy idempotent         | re-run I3                                                                       | Zero changed tasks (or only handler-bound)  |
| I5  | Authed request               | `curl -sSI -u "$ADMIN_USER:$LADDER_PASS" "https://ladder.<domain>/" \| head -1` | `HTTP/2 200`                                |
| I6  | Unauthed rejected            | `curl -sSI "https://ladder.<domain>/" \| head -1`                               | `HTTP/2 401`                                |
| I7  | Proxy works                  | `curl -sS -u … "https://ladder.<domain>/https://example.com/" \| grep -i title` | Returns example.com title                   |
| I8  | Ruleset loaded               | `journalctl -u ladder -n 50 --no-pager \| grep -i ruleset`                      | Confirms ruleset path/URL                   |
| I9  | Local ruleset override       | Set `ladder_ruleset_src`, redeploy, check journald                              | Path = `/etc/ladder/ruleset.yaml`           |
| I10 | Version sentinel idempotency | Re-deploy same version                                                          | Download/extract tasks skipped              |

### Lint / format gates

- `mise run lint` green before commit
- `mise run fmt` no diff before commit
- `mise run test` green before commit
- `hk` pre-commit hook enforces same — never `--no-verify`

### Flakiness / rate-limit notes

- GitHub `checksums.txt` fetch is one request per deploy on the control node — no rate-limit risk at normal cadence
- `caddy hash-password` produces fresh salt each run; templated Caddyfile always shows "would change" in `--check` mode (acceptable; matches colporteur)

### Not tested

- No molecule/testinfra harness — repo doesn't use one for any role
- No backup tests — feature explicitly skipped
- No FlareSolverr tests — feature explicitly skipped

---

## E) Commit List

Atomicity invariant: at every commit, `mise run test` and `mise run lint` are green; each commit is independently revertable.

### Phase 1 — Plan

**Commit 1**

- **Subject:** `docs(ladder): add deployment plan`
- **Layers:** docs
- **Changes:** `plan-ladder.md` (new)
- **Tests:** none

### Phase 2 — Ansible role scaffolding

**Commit 2**

- **Subject:** `feat(ladder): add ansible role for ladder http web proxy`
- **Layers:** infra
- **Changes:**
  - `ansible/roles/ladder/defaults/main.yml`
  - `ansible/roles/ladder/tasks/main.yml`
  - `ansible/roles/ladder/handlers/main.yml`
  - `ansible/roles/ladder/templates/ladder.service.j2`
  - `ansible/roles/ladder/templates/Caddyfile.j2`
- **Tests:** `ansible-lint roles/ladder` clean; rust tests unchanged

### Phase 3 — Config surface

**Commit 3**

- **Subject:** `feat(ladder): register dns subdomain and config keys`
- **Layers:** backend
- **Changes:**
  - `src/user_config.rs` — add `ladder_basic_auth_password`, `ladder_subdomain` to `TEMPLATE`
  - `config.example.toml` — mirror keys
  - `src/services/dns.rs` — add `"ladder_subdomain"` to `KNOWN_SUBDOMAIN_KEYS`
- **Tests:** U2, U3 (and U4 if no equivalent exists)

### Phase 4 — Wire into apps playbook

**Commit 4**

- **Subject:** `feat(ladder): register role in apps playbook`
- **Layers:** infra, backend
- **Changes:**
  - `ansible/playbooks/apps.yml` — alphabetical role entry, tags `[apps, web, proxy, ladder]`
  - `src/commands/deploy.rs` — extend `test_get_app_names_returns_roles` (U1)
- **Tests:** U1 passes; `mise run lint-ansible` green
- **Manual gate (off-tree):** run I1–I10 against the target VPS — must be 100% green before opening the PR

### Phase 5 — Documentation

**Commit 5**

- **Subject:** `docs(ladder): document deployment, config, and operations`
- **Layers:** docs
- **Changes:**
  - `docs/applications/apps/ladder.md` (new)
  - `docs/_sidebar.md` — entry under Apps
  - `docs/applications/overview.md` — entry
  - `README.md` — Apps bullet
- **Tests:** none

### Phase 6 — Final gates

Per planning rules §F. Not new commits unless review surfaces issues.

1. **Self PR-review pass** as a hostile reviewer; specifically scrutinize:
   - `caddy hash-password` task — no plaintext logging
   - Systemd hardening; optionally `systemd-analyze security ladder`
   - `EXPOSE_RULESET=false` actually takes effect (verify journald)
   - Idempotency on second deploy
2. **Apply review-driven changes** as additional follow-up commits — do not rebase-rewrite Phase 1–5 history
3. **Comment sweep** — remove any inline comments added during development (project's "no comments" rule)
4. **Open PR** — title `feat(ladder): add ladder http web proxy deployment`; body summarizes scope (auth model, version pinning, ruleset hybrid, no-flaresolverr, no-backup); "How tested" section listing I1–I10 results

### Sequencing summary

```
1. docs(ladder): add deployment plan
2. feat(ladder): add ansible role …
3. feat(ladder): register dns subdomain and config…
4. feat(ladder): register role in apps playbook        ← deploy gate (manual) before PR
5. docs(ladder): document deployment, config…
6. final review pass + comment sweep + open PR
```

5 commits → 1 PR. No `CHANGELOG.md` edit (release-please-managed).
