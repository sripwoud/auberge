# OPML Export/Import (FreshRSS)

Export and import RSS feed subscriptions independently of full backups.

## Export Feeds

```bash
auberge backup export-opml --host my-vps --output feeds.opml
```

With custom FreshRSS user:

```bash
auberge backup export-opml --host my-vps --output feeds.opml --user admin
```

## Import Feeds

```bash
auberge backup import-opml --host my-vps --input feeds.opml
```

This uploads the OPML file to the server and imports it into FreshRSS, then cleans up the temporary file.

## Use Cases

- Share feed subscriptions between hosts
- Backup feed lists without full database backup
- Migrate feeds to a fresh FreshRSS installation
- Import feeds from other RSS readers (if they support OPML export)

## Related Commands

- [backup create](creating-backups.md) - Full FreshRSS backup
- [backup restore](restoring-backups.md) - Restore FreshRSS from backup
- [CLI Reference: backup export-opml](../cli-reference/backup/export-opml.md) - Full export documentation
- [CLI Reference: backup import-opml](../cli-reference/backup/import-opml.md) - Full import documentation
