# Project Structure

```
auberge/
├── src/              # Rust CLI
├── ansible/          # Playbooks and roles
├── docs/             # Documentation
└── mise.toml         # Environment and tools
```

## Key Directories

**src/** - Rust CLI with commands in `src/commands/`

**ansible/playbooks/** - Layered playbooks (bootstrap → hardening → infrastructure → apps)

**ansible/roles/** - Ansible roles for each service

**docs/** - This documentation

## Data Locations

**Local:**

- `~/.config/auberge/hosts.toml` - Host configuration
- `~/.local/share/auberge/backups/` - Backup storage

**Remote:**

- `/var/lib/{service}/` - Application data
- `/etc/systemd/system/` - Service units
