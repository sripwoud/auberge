# auberge backup import-opml

Import feed subscriptions from an OPML file into FreshRSS. Alias: `auberge b io`.

```bash
auberge backup import-opml [OPTIONS] --input <FILE>
```

Uploads the OPML to `/tmp/` on the remote, runs the FreshRSS CLI import, then cleans up. Existing feeds are skipped (no duplicates). Categories are created if absent.

## Options

| Option               | Description         | Default                           |
| -------------------- | ------------------- | --------------------------------- |
| `-H, --host HOST`    | Target host         | Interactive                       |
| `-i, --input FILE`   | OPML file to import | Required                          |
| `-k, --ssh-key PATH` | SSH private key     | `~/.ssh/identities/{user}_{host}` |
| `--user USER`        | FreshRSS username   | `admin`                           |

## Examples

```bash
auberge backup import-opml --host myserver --input feeds.opml
auberge backup import-opml --host myserver --input feeds.opml --user john
```

## Gotchas

- Expects standard OPML 2.0 XML. Feed errors during import are reported but do not abort the import.
- If FreshRSS is not running: `ssh user@host 'systemctl status freshrss'`.
