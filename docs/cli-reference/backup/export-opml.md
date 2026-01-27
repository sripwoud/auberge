# auberge backup export-opml

Export FreshRSS feeds to OPML file

## Synopsis

```bash
auberge backup export-opml [OPTIONS] --output <FILE>
```

## Description

Exports FreshRSS feed subscriptions to an OPML file for backup or migration to other RSS readers.

OPML (Outline Processor Markup Language) is a standard format for RSS feed lists, compatible with most feed readers.

## Options

| Option             | Description                      | Default                         |
| ------------------ | -------------------------------- | ------------------------------- |
| -H, --host HOST    | Target host                      | Interactive selection           |
| -o, --output FILE  | Output OPML file path (required) | None                            |
| -k, --ssh-key PATH | SSH private key                  | ~/.ssh/identities/{user}_{host} |
| --user USER        | FreshRSS username                | admin                           |

## Examples

```bash
# Export feeds (interactive host selection)
auberge backup export-opml --output feeds.opml

# Specific host
auberge backup export-opml --host myserver --output freshrss-backup.opml

# Specific user
auberge backup export-opml --host myserver --output feeds.opml --user john

# Custom SSH key
auberge backup export-opml --host myserver --output feeds.opml --ssh-key ~/.ssh/custom_key
```

## Output Example

```
Using SSH key: ~/.ssh/identities/ansible_myserver
Exporting OPML from FreshRSS
  Host: myserver
  User: admin
  Output: feeds.opml

✓ OPML exported successfully
  Saved to: feeds.opml
```

## OPML File Format

```xml
<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head>
    <title>FreshRSS Feeds</title>
  </head>
  <body>
    <outline text="Tech" title="Tech">
      <outline type="rss" text="Hacker News" xmlUrl="https://news.ycombinator.com/rss"/>
      <outline type="rss" text="TechCrunch" xmlUrl="https://techcrunch.com/feed/"/>
    </outline>
    <outline text="News" title="News">
      <outline type="rss" text="BBC News" xmlUrl="http://feeds.bbci.co.uk/news/rss.xml"/>
    </outline>
  </body>
</opml>
```

## Use Cases

**Backup**: Regular export of feed subscriptions

```bash
auberge backup export-opml --output ~/backups/freshrss-$(date +%Y-%m-%d).opml
```

**Migration**: Export from one server to import on another

```bash
# Export from old server
auberge backup export-opml --host oldserver --output feeds.opml

# Import to new server
auberge backup import-opml --host newserver --input feeds.opml
```

**Portability**: Import into other RSS readers (Feedly, Inoreader, etc.)

## Related Commands

- [auberge backup import-opml](import-opml.md) - Import OPML file to FreshRSS
- [auberge backup create](create.md) - Full FreshRSS backup

## See Also

- [FreshRSS](../../applications/freshrss.md)
- [Backup and Restore](../../backup-restore/README.md)
