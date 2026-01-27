# Infrastructure Layers

Detailed breakdown of each infrastructure layer, their components, dependencies, and interactions.

## Layer Dependencies

```
┌─────────────────────────────────────────┐
│  Layer 4: Applications (apps.yml)       │
│  - Blocky, Radicale, FreshRSS, etc.     │
└─────────────────┬───────────────────────┘
                  │ depends on
┌─────────────────▼───────────────────────┐
│  Layer 3: Infrastructure                │
│  - Caddy, APT, Bash                     │
└─────────────────┬───────────────────────┘
                  │ depends on
┌─────────────────▼───────────────────────┐
│  Layer 2: Hardening                     │
│  - fail2ban, kernel hardening           │
└─────────────────┬───────────────────────┘
                  │ depends on
┌─────────────────▼───────────────────────┐
│  Layer 1: Bootstrap                     │
│  - Users, SSH, UFW                      │
└─────────────────────────────────────────┘
```

## Layer 1: Bootstrap

### Purpose

Establishes secure foundation for all subsequent automation.

### Critical Components

#### Hostname Configuration

Sets the VPS hostname to match inventory name:

```yaml
- name: Set hostname to inventory name
  ansible.builtin.hostname:
    name: "{{ inventory_hostname }}"
```

**Why:** Ensures consistent identification across backups, logs, and SSH.

#### Dual-User System

Creates two users with different roles:

**ansible (automation user):**

- Created by `ansible_user` role
- SSH public key deployed from local machine
- Passwordless sudo access
- Used for all Ansible operations

**{admin_user_name} (personal admin):**

- Created by `user` role
- SSH public key deployed
- Full sudo privileges
- Used for manual administration

**Why:** Separation of concerns - automation vs manual access.

#### SSH Hardening

Configures SSH for security:

```yaml
# Key settings in sshd_config
Port: {{ lookup('env', 'SSH_PORT') }}
PermitRootLogin: no
PasswordAuthentication: no
PubkeyAuthentication: yes
```

**Validation process:**

1. Apply SSH config changes
2. Test new connection on custom port
3. Only then enable firewall

**Why:** Prevents lockout from SSH configuration errors.

#### UFW Firewall

Sets up firewall rules but doesn't enable until SSH is validated:

```yaml
# Rules configured
- Allow custom SSH port
- Allow 80/tcp (HTTP)
- Allow 443/tcp (HTTPS)
- Allow 853/tcp (DNS over TLS)
- Default deny incoming
- Default allow outgoing
```

**Activation:** Only after SSH validation succeeds.

**Why:** Avoids locking yourself out if SSH config has errors.

### Prerequisites

- Root SSH access to VPS
- `SSH_PORT` environment variable set
- Provider firewall allows custom SSH port

### Post-Bootstrap State

- ✓ ansible user can SSH with key
- ✓ admin user can SSH with key
- ✓ Root login disabled
- ✓ Password auth disabled
- ✓ UFW firewall active
- ✓ SSH running on custom port

## Layer 2: Hardening

### Purpose

Apply security best practices to the system.

### Components

#### fail2ban

Intrusion prevention system that bans IPs after failed login attempts.

**Jails configured:**

- `sshd` - SSH brute-force protection
- `caddy-*` - HTTP auth failures
- Custom application jails

**Configuration:**

```ini
# /etc/fail2ban/jail.d/defaults.conf
bantime = 1h
findtime = 10m
maxretry = 5
```

**Why:** Automated defense against brute-force attacks.

#### Kernel Hardening

Applies sysctl parameters for security:

```yaml
# Key parameters
net.ipv4.conf.all.rp_filter: 1 # Reverse path filtering
net.ipv4.conf.default.rp_filter: 1
net.ipv4.icmp_echo_ignore_broadcasts: 1 # Ignore ICMP broadcasts
net.ipv4.conf.all.accept_source_route: 0 # No source routing
net.ipv4.conf.all.send_redirects: 0 # No ICMP redirects
kernel.dmesg_restrict: 1 # Restrict dmesg
kernel.kptr_restrict: 2 # Hide kernel pointers
```

**Why:** Harden kernel against common attack vectors.

### Dependencies

- Requires ansible user (from bootstrap)
- Requires SSH on custom port (for fail2ban SSH jail)

### Post-Hardening State

- ✓ fail2ban active and monitoring
- ✓ Kernel parameters hardened
- ✓ System logs protected

## Layer 3: Infrastructure

### Purpose

Install and configure core services required by applications.

### Components

#### APT (Package Management)

Configures system package management:

**Responsibilities:**

- Enable unattended-upgrades for security patches
- Configure apt sources
- Install essential packages (curl, git, etc.)

**Key packages installed:**

- `curl`, `wget` - Download utilities
- `git` - Version control
- `vim` - Text editor
- `htop` - Process viewer
- `rsync` - Backup utility
- Build essentials as needed

**Why:** Ensures consistent base system across all VPS hosts.

#### Bash

Configures shell environment for admin user:

**Responsibilities:**

- Deploy `.bashrc` with aliases
- Set up bash completion
- Configure command history
- Set environment variables

**Example customizations:**

```bash
# Aliases
alias ll='ls -lah'
alias g='git'

# History settings
HISTSIZE=10000
HISTFILESIZE=20000
```

**Why:** Improves command-line productivity for manual administration.

#### Caddy

Reverse proxy with automatic HTTPS via Let's Encrypt.

**Responsibilities:**

- Install Caddy binary
- Create systemd service
- Generate Caddyfile from templates
- Configure automatic HTTPS
- Set up reverse proxy routes for all apps

**Caddyfile structure:**

```
{subdomain}.{domain} {
    reverse_proxy localhost:{app_port}
    tls {
        dns cloudflare {env.CLOUDFLARE_DNS_API_TOKEN}
    }
}
```

**Why:**

- Automatic SSL/TLS certificates
- Single reverse proxy for all services
- DNS-01 ACME challenges work behind NAT

**Dependencies:**

- Cloudflare API token (for DNS-01 challenges)
- Port 80/443 open in firewall

### Post-Infrastructure State

- ✓ System packages up to date
- ✓ Bash environment configured
- ✓ Caddy running and serving HTTPS
- ✓ SSL certificates provisioned

## Layer 4: Applications

### Purpose

Deploy self-hosted applications on top of infrastructure.

### Application Categories

#### Network Services

**Blocky (DNS + Ad-blocking):**

- Port: 853 (DNS over TLS)
- Subdomain: `dns.{domain}`
- Data: `/etc/blocky/`
- Purpose: Private DNS with ad/tracking blocking

**WireGuard (VPN):**

- Port: 51820/udp
- Config: `/etc/wireguard/`
- Purpose: Secure remote access

#### Storage & Media

**Radicale (CalDAV/CardDAV):**

- Port: 5232
- Subdomain: `cal.{domain}`
- Data: `/var/lib/radicale/`
- Purpose: Calendar and contact sync

**Navidrome (Music):**

- Port: 4533
- Subdomain: `music.{domain}`
- Data: `/var/lib/navidrome/`, `/srv/music/`
- Purpose: Music streaming server

**Calibre (Ebooks):**

- Port: 8083
- Subdomain: `books.{domain}`
- Data: `/home/calibre/`
- Purpose: Ebook library management

**WebDAV (File Sharing):**

- Port: 8080
- Subdomain: `files.{domain}`
- Data: `/var/lib/webdav/`
- Purpose: File sharing and sync

#### Web Applications

**FreshRSS (RSS Reader):**

- Port: 8000
- Subdomain: `rss.{domain}`
- Data: `/var/lib/freshrss/`
- Purpose: RSS feed aggregator

**YOURLS (URL Shortener):**

- Port: 8081
- Subdomain: `url.{domain}`
- Data: `/var/lib/yourls/`
- Purpose: Self-hosted URL shortener

### Common Pattern

Each application follows the same deployment pattern:

1. **Install binary or source code**
   - Download from GitHub releases
   - Or install via package manager
   - Or clone repository

2. **Create service user** (if needed)
   ```yaml
   - name: Create service user
     ansible.builtin.user:
       name: "{{ service_name }}"
       system: true
       shell: /usr/sbin/nologin
   ```

3. **Create data directories**
   ```yaml
   - name: Create data directory
     ansible.builtin.file:
       path: "/var/lib/{{ service_name }}"
       state: directory
       owner: "{{ service_name }}"
       group: "{{ service_name }}"
       mode: "0755"
   ```

4. **Deploy configuration**
   ```yaml
   - name: Deploy config
     ansible.builtin.template:
       src: config.yml.j2
       dest: "/etc/{{ service_name }}/config.yml"
       owner: "{{ service_name }}"
       group: "{{ service_name }}"
       mode: "0644"
     notify: Restart service
   ```

5. **Create systemd service**
   ```yaml
   - name: Deploy systemd service
     ansible.builtin.template:
       src: service.j2
       dest: "/etc/systemd/system/{{ service_name }}.service"
       owner: root
       group: root
       mode: "0644"
     notify:
       - Reload systemd
       - Restart service
   ```

6. **Enable and start service**
   ```yaml
   - name: Enable and start service
     ansible.builtin.systemd_service:
       name: "{{ service_name }}"
       enabled: true
       state: started
       daemon_reload: true
   ```

7. **Add Caddy route** (if web-accessible)
   - Caddy config is regenerated with all app routes
   - Automatic HTTPS via Let's Encrypt

### Dependencies

**All applications require:**

- Caddy (reverse proxy)
- APT packages (base system)
- UFW ports open (80, 443, app-specific)

**Application-specific:**

- Navidrome → music files in `/srv/music/`
- YOURLS → MySQL/MariaDB database
- FreshRSS → SQLite (bundled)

### Post-Application State

- ✓ All services running via systemd
- ✓ HTTPS endpoints accessible
- ✓ Data directories created with correct permissions
- ✓ Automatic service restart on boot

## Layer Interactions

### Bootstrap → Hardening

```
ansible user created (bootstrap)
    ↓
Used for all subsequent plays (hardening)
```

### Hardening → Infrastructure

```
System secured (hardening)
    ↓
Safe to install services (infrastructure)
```

### Infrastructure → Applications

```
Caddy configured (infrastructure)
    ↓
Apps register routes with Caddy (applications)
    ↓
Caddy provisions SSL certs
    ↓
Apps accessible via HTTPS
```

### Caddy + Applications

All applications integrate with Caddy:

```yaml
# App role includes Caddy config snippet
- name: Add Caddy route
  ansible.builtin.template:
    src: caddy-route.j2
    dest: "/etc/caddy/conf.d/{{ service_name }}.conf"
  notify: Reload caddy
```

Caddy aggregates all routes into unified config.

## Idempotency Across Layers

Each layer is idempotent:

```bash
# First run: makes changes
auberge ansible run --tags infrastructure
# → Changed: 15  Ok: 5

# Second run: no changes needed
auberge ansible run --tags infrastructure
# → Changed: 0  Ok: 20
```

This applies to:

- File deployments (only changes if content differs)
- Package installations (only installs if missing)
- Service management (only restarts if config changed)

## Running Individual Layers

```bash
# Bootstrap only (first time)
auberge ansible bootstrap my-vps --ip 194.164.53.11

# Hardening only
auberge ansible run --tags hardening

# Infrastructure only
auberge ansible run --tags infrastructure

# Apps only
auberge ansible run --tags apps

# Specific app
auberge ansible run --tags radicale

# Multiple layers
auberge ansible run --tags hardening,infrastructure
```

## Related Pages

- [Architecture](core-concepts/architecture.md) - High-level system design
- [Playbooks](core-concepts/playbooks.md) - Playbook structure
- [Running Playbooks](deployment/running-playbooks.md) - Execution guide
- [Tags](deployment/tags.md) - Complete tag reference
