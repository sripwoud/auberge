# auberge backup export-opml

Export FreshRSS feed subscriptions to an OPML file. Alias: `auberge b eo`.

```bash
auberge backup export-opml [OPTIONS] --output <FILE>
```

## Options

| Option               | Description           | Default                           |
| -------------------- | --------------------- | --------------------------------- |
| `-H, --host HOST`    | Source host           | Interactive                       |
| `-o, --output FILE`  | Output OPML file path | Required                          |
| `-k, --ssh-key PATH` | SSH private key       | `~/.ssh/identities/{user}_{host}` |
| `--user USER`        | FreshRSS username     | `admin`                           |

## Examples

```bash
auberge backup export-opml --host myserver --output feeds.opml
auberge backup export-opml --host myserver --output ~/backups/freshrss-$(date +%Y-%m-%d).opml
# migrate to new server
auberge backup export-opml --host oldserver --output feeds.opml
auberge backup import-opml --host newserver --input feeds.opml
```
