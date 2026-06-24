# Common Issues

## Installation

| Error                         | Cause                   | Fix                             |
| ----------------------------- | ----------------------- | ------------------------------- |
| `cargo install auberge` fails | Outdated Rust toolchain | Run `rustup update`, then retry |
| `mise not found`              | mise not installed      | `curl https://mise.run          |

## Configuration

### `"Missing required config value"`

```bash
auberge config list
auberge config set KEY value
```

## Host Management

| Error                                  | Cause                                           | Fix                                           |
| -------------------------------------- | ----------------------------------------------- | --------------------------------------------- |
| `"Host not found"`                     | Host absent from `~/.config/auberge/hosts.toml` | `auberge host add my-vps 203.0.113.10`        |
| `~/.config/auberge/hosts.toml` missing | Directory not created                           | `mkdir -p ~/.config/auberge`, then add a host |

## Backup

| Error                | Cause                    | Fix                                   |
| -------------------- | ------------------------ | ------------------------------------- |
| `"No backups found"` | No backup created yet    | `auberge backup create --host my-vps` |
| Backup hangs         | Stale SSH control socket | `rm -rf ~/.ssh/ctl-*`, then retry     |

## Deployment

| Error                        | Cause                                | Fix                                                                                                     |
| ---------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| `"Unreachable"`              | SSH connectivity failure             | See [SSH Problems](troubleshooting/ssh-problems.md)                                                                     |
| `"Permission denied"` (sudo) | ansible user lacks passwordless sudo | `ssh ansible@vps "sudo -n true"`; re-run `auberge ansible bootstrap my-vps --ip 203.0.113.10` if needed |
| Handler not running          | Config task reported no change       | Restart manually: `ssh ansible@vps "sudo systemctl restart service-name"`                               |

## Application

### Service won't start

```bash
ssh ansible@vps "systemctl status service-name"
ssh ansible@vps "journalctl -u service-name -n 50"
```

Check: config syntax, port conflicts (`sudo ss -tulpn`), file permissions.

### HTTPS certificate errors

```bash
ssh ansible@vps "journalctl -u caddy -n 50"
ssh ansible@vps "sudo systemctl restart caddy"
```

Causes: Cloudflare API token wrong, DNS not pointing to VPS, port 80/443 blocked. See [DNS Issues](troubleshooting/dns-issues.md).

## Performance

| Problem           | Fix                                                                                     |
| ----------------- | --------------------------------------------------------------------------------------- |
| Slow playbook     | Use `auberge ansible run --tags specific-app` to scope execution                        |
| Large backup size | Music excluded by default; check sizes with `du -sh ~/.local/share/auberge/backups/*/*` |
