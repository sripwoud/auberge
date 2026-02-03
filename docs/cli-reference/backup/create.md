# auberge backup create

Create timestamped backup of application data

## Usage

```bash
auberge backup create [OPTIONS]
# Alias: auberge b c
```

## Options

- `-H, --host` - Target host (interactive if omitted)
- `-a, --apps` - Apps to backup (comma-separated, default: all)
- `-d, --dest` - Destination directory (default: ~/.local/share/auberge/backups)
- `--include-music` - Include music files in Navidrome backup
- `-n, --dry-run` - Preview without backing up
- `-v, --verbose` - Show detailed progress

## Examples

```bash
# Interactive
auberge backup create

# Specific apps
auberge backup create --host myserver --apps baikal,freshrss

# Include music (large)
auberge backup create --host myserver --apps navidrome --include-music

# Dry run
auberge backup create --host myserver --dry-run
```

Backup structure: `~/.local/share/auberge/backups/{host}/{timestamp}/{app}/`

Services are stopped during backup, then restarted.

Use `auberge backup create --help` for full options.
