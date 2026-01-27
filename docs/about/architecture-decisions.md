# Architecture Decisions

Why Auberge is built the way it is.

## No Docker

**Decision:** Use systemd services instead of containers.

**Reasoning:**

- **Memory efficiency:** 1GB VPS can't spare 200-300MB for Docker daemon
- **Simplicity:** No container orchestration complexity
- **Performance:** Native binaries, no virtualization overhead
- **Debugging:** Standard systemd tools (journalctl, systemctl)
- **Security:** Smaller attack surface, no container escapes

**Trade-offs:**

- Less portable across different Linux distributions
- No application isolation (mitigated by systemd user isolation)
- Manual dependency management

## Rust CLI

**Decision:** Build CLI in Rust instead of shell scripts or Python.

**Reasoning:**

- **Type safety:** Catch errors at compile time
- **Performance:** Fast execution, low overhead
- **Single binary:** Easy distribution via `cargo install`
- **Cross-platform:** Works on Linux, macOS, Windows
- **Memory safety:** No segfaults or buffer overflows

**Trade-offs:**

- Slower iteration (compilation time)
- Learning curve for contributors
- Larger binary size than scripts

## Ansible for Configuration

**Decision:** Use Ansible instead of writing custom provisioning code.

**Reasoning:**

- **Idempotent:** Safe to run multiple times
- **Declarative:** Describe desired state, not steps
- **Mature ecosystem:** Extensive modules and community
- **YAML syntax:** Readable and easy to modify
- **Agentless:** No daemon on VPS

**Trade-offs:**

- Learning curve for Ansible
- YAML verbosity
- Python dependency (but already on most Linux systems)

## Dual-User System

**Decision:** Create separate ansible (automation) and admin (personal) users.

**Reasoning:**

- **Separation of concerns:** Automation vs manual access
- **Security:** Ansible user only for deployments
- **Auditability:** Clear distinction in logs
- **Flexibility:** Admin user for emergencies

**Trade-offs:**

- More complex initial setup
- Two SSH keys to manage
- Slightly more disk space

## SSH Key Derivation

**Decision:** Use pattern `~/.ssh/identities/{user}_{hostname}` for default keys.

**Reasoning:**

- **Predictable:** Easy to find keys
- **Organized:** All keys in one directory
- **Naming clarity:** User and host explicit in filename
- **Override-able:** Can specify custom keys

**Trade-offs:**

- Non-standard location (most use `~/.ssh/id_rsa`)
- Directory structure required

## hosts.toml vs inventory.yml

**Decision:** Maintain two separate host configuration systems.

**Reasoning:**

- **hosts.toml:** User-specific, not version controlled, for backup operations
- **inventory.yml:** Shared, version controlled, for deployments
- **Flexibility:** Different use cases require different approaches
- **Privacy:** Don't commit personal IPs to repository

**Trade-offs:**

- Duplication of host information
- Potential for sync issues
- More complex mental model

## age Encryption for Secrets

**Decision:** Use age instead of GPG or HashiCorp Vault.

**Reasoning:**

- **Simple:** Modern, minimal API
- **Secure:** Based on modern cryptography (X25519, ChaCha20)
- **Fast:** Asymmetric encryption with good performance
- **Small:** Single binary, no dependencies
- **Auditable:** Short, readable code

**Trade-offs:**

- Less widespread than GPG
- Not hardware security module (HSM) compatible
- Newer, less battle-tested than GPG

## Cloudflare for DNS

**Decision:** Hard dependency on Cloudflare (not provider-agnostic).

**Reasoning:**

- **DNS-01 ACME:** Required for certificates behind NAT
- **API simplicity:** Easy to use, well-documented
- **Free tier:** Generous limits for personal use
- **DDoS protection:** Built-in if proxying enabled
- **Global DNS:** Fast propagation

**Trade-offs:**

- Vendor lock-in
- Single point of failure
- Privacy concerns (Cloudflare sees DNS queries)

## Backup Strategy

**Decision:** Use rsync for backups, store locally.

**Reasoning:**

- **Simplicity:** rsync is ubiquitous and reliable
- **Incremental:** Only transfer changed data
- **Local storage:** Full control, no cloud dependency
- **Fast restore:** No download wait time
- **Compression:** Automatic via rsync

**Trade-offs:**

- No off-site backup by default
- Local machine becomes single point of failure
- Manual management of retention policy

## systemd for Service Management

**Decision:** systemd as service manager (not init.d or supervisord).

**Reasoning:**

- **Standard:** Default on modern Linux distributions
- **Features:** Auto-restart, dependencies, logging
- **Integration:** Works with journalctl for logs
- **Resource limits:** CPU, memory limiting built-in

**Trade-offs:**

- Not portable to non-systemd systems (Alpine, etc.)
- More complex than simple init scripts

## Monorepo Structure

**Decision:** Single repository for CLI, Ansible, and documentation.

**Reasoning:**

- **Atomic changes:** CLI and playbooks updated together
- **Simpler CI/CD:** One pipeline, one release
- **Documentation co-location:** Docs alongside code
- **Version synchronization:** No version skew

**Trade-offs:**

- Larger repository
- Mixed concerns (Rust + Ansible)
- Clone includes everything (can't partial clone)

## Why AGPL-3.0

**Decision:** Use AGPL-3.0 license instead of MIT or Apache-2.0.

**Reasoning:**

- **Copyleft:** Modifications must be shared
- **Network clause:** SaaS providers must share source
- **FOSS ethos:** Keeps software free
- **Anti-commercialization:** Prevents proprietary forks

**Trade-offs:**

- Less permissive than MIT/Apache
- Corporate adoption barriers
- Legal complexity

## Design Principles

### Simplicity Over Features

Auberge prioritizes:

- ✓ Simple, understandable code
- ✓ Minimal dependencies
- ✓ Clear error messages

Over:

- ✗ Feature richness
- ✗ Configurability
- ✗ Flexibility

### Opinionated Defaults

Auberge makes choices:

- Single reverse proxy (Caddy)
- Single DNS provider (Cloudflare)
- Single VPN (WireGuard)

**Why:** Reduces maintenance burden, improves documentation, easier testing.

### Fail Fast

Auberge prefers:

- ✓ Clear errors at start
- ✓ Pre-flight validation
- ✓ Explicit confirmations

Over:

- ✗ Trying to recover automatically
- ✗ Silent failures
- ✗ Assuming user intent

## Related Pages

- [Architecture](core-concepts/architecture.md) - System design
- [Roadmap](about/roadmap.md) - Future direction
- [Contributing](development/contributing.md) - How to contribute
