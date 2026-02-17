# Playbooks

Layered playbooks that build on each other.

## Layers

### bootstrap.yml

Initial VPS setup (root user → ansible user)

Creates users, configures SSH, sets up firewall

```bash
auberge ansible bootstrap my-vps --ip 203.0.113.10
```

### hardening.yml

Security layer (fail2ban, kernel hardening)

```bash
auberge ansible run --tags hardening
```

### infrastructure.yml

Core services (apt, bash, Caddy)

```bash
auberge ansible run --tags infrastructure
```

### apps.yml

Self-hosted applications

```bash
auberge ansible run --tags apps
# Or specific apps
auberge ansible run --tags baikal,freshrss
```

### auberge.yml

Master playbook (runs all layers)

```bash
auberge ansible run --playbook playbooks/auberge.yml
```

## Tags

Use tags for selective execution:

```bash
auberge ansible run --tags ssh              # Only SSH config
auberge ansible run --tags apps             # All apps
auberge ansible run --tags baikal         # Single app
auberge ansible run --skip-tags navidrome   # Exclude app
```

## Check Mode

Dry run before execution:

```bash
auberge ansible check --tags myapp
```

## Dependencies

```
bootstrap → hardening → infrastructure → apps
```

Bootstrap must run first. Other layers can be re-run independently.
