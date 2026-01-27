# auberge backup import-opml

Import OPML file to FreshRSS

## Synopsis

```bash
auberge backup import-opml [OPTIONS] --input <FILE>
```

## Description

Imports feed subscriptions from an OPML file into FreshRSS. Useful for:

- Restoring feed subscriptions from backup
- Migrating from another RSS reader
- Bulk adding feeds

## Options

| Option             | Description                    | Default                         |
| ------------------ | ------------------------------ | ------------------------------- |
| -H, --host HOST    | Target host                    | Interactive selection           |
| -i, --input FILE   | OPML file to import (required) | None                            |
| -k, --ssh-key PATH | SSH private key                | ~/.ssh/identities/{user}_{host} |
| --user USER        | FreshRSS username              | admin                           |

## Examples

```bash
# Import feeds (interactive host selection)
auberge backup import-opml --input feeds.opml

# Specific host
auberge backup import-opml --host myserver --input freshrss-backup.opml

# Specific user
auberge backup import-opml --host myserver --input feeds.opml --user john

# Custom SSH key
auberge backup import-opml --host myserver --input feeds.opml --ssh-key ~/.ssh/custom_key
```

## Process

1. **Upload OPML file** to /tmp/ on remote host
2. **Run FreshRSS CLI import** command
3. **Clean up** temporary file
4. **Display results**

## Output Example

```
Using SSH key: ~/.ssh/identities/ansible_myserver
Importing OPML to FreshRSS
  Host: myserver
  User: admin
  Input: feeds.opml

  Uploading OPML file...
  Importing feeds...

Imported 47 feeds in 3 categories
✓ OPML imported successfully
```

## Supported OPML Format

Standard OPML 2.0 format:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>Feed List</title></head>
  <body>
    <outline text="Category Name">
      <outline type="rss" text="Feed Name" xmlUrl="https://example.com/feed"/>
    </outline>
  </body>
</opml>
```

## Import Behavior

- **Existing feeds**: Skipped (not duplicated)
- **Categories**: Created if they don't exist
- **Feed errors**: Reported but don't stop import

## Use Cases

**Restore from export**:

```bash
# Export from old server
auberge backup export-opml --host oldserver --output feeds.opml

# Import to new server
auberge backup import-opml --host newserver --input feeds.opml
```

**Migrate from other readers**: Export OPML from Feedly, Inoreader, etc., then import

**Bulk add feeds**: Create OPML file with multiple feeds and import at once

## Troubleshooting

**File not found**:

- Check file path is correct
- Use absolute path or ensure file is in current directory

**Import fails**:

- Verify OPML format is valid XML
- Check FreshRSS is running: `ssh user@host 'systemctl status freshrss'`
- Check FreshRSS logs: `ssh user@host 'journalctl -u freshrss'`

**User not found**:

- Verify username exists in FreshRSS
- Default user is 'admin'

## Related Commands

- [auberge backup export-opml](export-opml.md) - Export FreshRSS feeds
- [auberge backup restore](restore.md) - Full FreshRSS restore

## See Also

- [FreshRSS](../../applications/freshrss.md)
- [Backup and Restore](../../backup-restore/README.md)
