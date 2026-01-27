# Tags

Tags allow selective execution of specific parts of playbooks without running everything.

## Overview

**Purpose:** Run only specific tasks/roles without executing entire playbook

**Syntax:**

```bash
auberge ansible run --tags tag1,tag2,tag3
auberge ansible run --skip-tags tag1,tag2
```

**Benefit:** Faster deployments, targeted updates

## Tag Categories

Tags in Auberge are organized by:

- **Layer** (bootstrap, hardening, infrastructure, apps)
- **Component** (specific services/apps)
- **Category** (security, network, storage)

### Layer Tags

Run entire layers:

```bash
# Bootstrap layer only
auberge ansible run --tags bootstrap

# Hardening layer only
auberge ansible run --tags hardening

# Infrastructure layer only
auberge ansible run --tags infrastructure

# Applications layer only
auberge ansible run --tags apps
```

### Component Tags

#### Infrastructure Components

```bash
# SSH configuration
auberge ansible run --tags ssh

# Firewall (UFW)
auberge ansible run --tags ufw
auberge ansible run --tags firewall  # Alias

# Reverse proxy
auberge ansible run --tags caddy

# Package management
auberge ansible run --tags apt

# Shell environment
auberge ansible run --tags bash

# Intrusion prevention
auberge ansible run --tags fail2ban

# Kernel hardening
auberge ansible run --tags kernel_hardening
```

#### Application Components

```bash
# DNS + ad-blocking
auberge ansible run --tags blocky

# Calendar/contacts
auberge ansible run --tags radicale

# RSS reader
auberge ansible run --tags freshrss

# Music streaming
auberge ansible run --tags navidrome

# Ebook library
auberge ansible run --tags calibre

# File sharing
auberge ansible run --tags webdav

# VPN
auberge ansible run --tags wireguard

# URL shortener
auberge ansible run --tags yourls
```

### Category Tags

Group related components:

```bash
# All security-related tasks
auberge ansible run --tags security

# All network-related tasks
auberge ansible run --tags network

# All storage/media apps
auberge ansible run --tags storage

# All web applications
auberge ansible run --tags web
```

## Tag Combinations

### Multiple Tags (OR Logic)

Run tasks with **any** of the specified tags:

```bash
# Run Radicale OR FreshRSS OR Navidrome
auberge ansible run --tags radicale,freshrss,navidrome
```

**Result:** All three applications deploy.

### Skip Tags

Run everything **except** specified tags:

```bash
# Run full stack but skip bootstrap
auberge ansible run --playbook playbooks/auberge.yml --skip-tags bootstrap

# Run all apps except Navidrome
auberge ansible run --tags apps --skip-tags navidrome
```

### Combine Tags and Skip-Tags

```bash
# Run only apps, but skip Calibre
auberge ansible run --tags apps --skip-tags calibre
```

## Common Patterns

### Update Single Application

```bash
# Update only Radicale
auberge ansible run --host auberge --tags radicale
```

**What runs:**

- Radicale installation
- Radicale configuration
- Radicale systemd service
- Radicale data directory setup
- Service restart (if config changed)

**What doesn't run:**

- Other applications
- Infrastructure layer
- Security layer

### Update Multiple Related Apps

```bash
# Update all media/storage apps
auberge ansible run --tags radicale,navidrome,calibre,webdav
```

### Update Infrastructure + One App

```bash
# Update Caddy and Radicale (e.g., after changing Caddy route)
auberge ansible run --tags caddy,radicale
```

### Security Updates Only

```bash
# Apply all security-related changes
auberge ansible run --tags security
```

**Includes:**

- fail2ban
- UFW
- SSH hardening
- Kernel hardening

### Full Deployment (Skip Bootstrap)

```bash
# Common for already-bootstrapped systems
auberge ansible run --playbook playbooks/auberge.yml --skip-tags bootstrap
```

**Why:** Bootstrap is one-time setup. Skipping prevents errors/conflicts.

## Tag Hierarchy

Some tags inherit others:

```
apps
├── blocky
├── calibre
├── freshrss
├── navidrome
├── radicale
├── webdav
├── wireguard
└── yourls
```

Running `--tags apps` executes all child application tags.

```
security
├── fail2ban
├── kernel_hardening
├── ufw
└── ssh
```

Running `--tags security` executes all security-related tasks.

## Tag Reference

### Complete Tag List

| Tag                | Layer     | Description          |
| ------------------ | --------- | -------------------- |
| `bootstrap`        | Layer     | Initial VPS setup    |
| `hardening`        | Layer     | Security hardening   |
| `infrastructure`   | Layer     | Core services        |
| `apps`             | Layer     | All applications     |
| `ssh`              | Component | SSH configuration    |
| `ufw`, `firewall`  | Component | UFW firewall         |
| `caddy`            | Component | Reverse proxy        |
| `apt`              | Component | Package management   |
| `bash`             | Component | Shell environment    |
| `fail2ban`         | Component | Intrusion prevention |
| `kernel_hardening` | Component | Kernel security      |
| `blocky`           | App       | DNS + ad-blocking    |
| `radicale`         | App       | CalDAV/CardDAV       |
| `freshrss`         | App       | RSS reader           |
| `navidrome`        | App       | Music streaming      |
| `calibre`          | App       | Ebook library        |
| `webdav`           | App       | File sharing         |
| `wireguard`        | App       | VPN                  |
| `yourls`           | App       | URL shortener        |
| `security`         | Category  | All security tasks   |
| `network`          | Category  | All network tasks    |
| `storage`          | Category  | All storage tasks    |
| `web`              | Category  | All web apps         |
| `users`            | Category  | User management      |
| `system`           | Category  | System configuration |

## Viewing Available Tags

List all tags in a playbook:

```bash
ansible-playbook playbooks/auberge.yml --list-tags
```

**Example output:**

```
playbook: playbooks/auberge.yml

  play #1 (vps): Bootstrap
    TAGS: [bootstrap, ssh, ufw]

  play #2 (vps): Hardening
    TAGS: [fail2ban, hardening, kernel_hardening, security]

  play #3 (vps): Infrastructure
    TAGS: [apt, bash, caddy, infrastructure]

  play #4 (vps): Applications
    TAGS: [apps, blocky, calibre, freshrss, navidrome, radicale, webdav, wireguard, yourls]
```

## How Tags Work

### Task-Level Tags

Most tasks have explicit tags:

```yaml
- name: Install Caddy
  ansible.builtin.get_url:
    url: https://...
    dest: /usr/local/bin/caddy
  tags: [infrastructure, caddy, web]
```

**Running `--tags caddy`:** This task executes

**Running `--tags infrastructure`:** This task executes

**Running `--tags apps`:** This task skipped

### Role-Level Tags

Roles are tagged when included:

```yaml
- role: radicale
  tags: [apps, radicale, storage, caldav]
```

**All tasks in the role inherit these tags.**

### Play-Level Tags

Entire plays can be tagged:

```yaml
- name: Bootstrap
  hosts: vps
  tags: [bootstrap]
  roles:
    - ansible_user
    - ssh
```

**All tasks and roles in the play inherit the tag.**

## Best Practices

### Use Tags for Incremental Updates

```bash
# Changed Radicale config → update only Radicale
auberge ansible run --tags radicale

# Not necessary to run entire apps.yml
```

**Benefit:** Faster feedback loop during development.

### Always Skip Bootstrap in Production

```bash
# Good
auberge ansible run --playbook playbooks/auberge.yml --skip-tags bootstrap

# Bad (may cause errors)
auberge ansible run --playbook playbooks/auberge.yml
```

**Why:** Bootstrap creates users and changes SSH - should only run once.

### Combine with Check Mode

```bash
# Preview what specific tag would change
auberge ansible check --tags radicale

# If safe, apply
auberge ansible run --tags radicale
```

### Tag Application Groups

Update related apps together:

```bash
# All media apps
auberge ansible run --tags navidrome,calibre

# All productivity apps
auberge ansible run --tags radicale,freshrss
```

### Use Layer Tags for Major Updates

```bash
# Update all applications
auberge ansible run --tags apps

# Update all infrastructure
auberge ansible run --tags infrastructure
```

## Troubleshooting

### "No tasks matched"

No tasks have the specified tag.

**Fix:** Check tag spelling:

```bash
ansible-playbook playbooks/apps.yml --list-tags
```

### Unexpected Tasks Run

Task has multiple tags and matches your filter.

**Example:**

```yaml
- name: Configure UFW
  tags: [bootstrap, hardening, firewall, ufw]
```

Running `--tags hardening` includes this task (even though it's in bootstrap).

**Solution:** Use `--skip-tags` to exclude:

```bash
auberge ansible run --tags hardening --skip-tags bootstrap
```

### Handlers Not Running

Handlers only run if notifying task executed.

**Example:**

```bash
# Run only specific app
auberge ansible run --tags radicale
```

If Radicale config didn't change, restart handler won't run.

**Solution:** Manually restart if needed:

```bash
ssh ansible@auberge "sudo systemctl restart radicale"
```

## Related Pages

- [Running Playbooks](deployment/running-playbooks.md) - Execution guide
- [Check Mode](deployment/check-mode.md) - Dry-run testing
- [Playbooks](core-concepts/playbooks.md) - Playbook structure
- [Infrastructure Layers](core-concepts/infrastructure-layers.md) - Layer dependencies
