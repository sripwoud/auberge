# auberge select host

Interactively select a host and print its name to stdout. Alias: `auberge se h`.

```bash
auberge select host [OPTIONS]
```

Most commands invoke this automatically when `--host` is omitted. Use directly for shell scripting.

## Options

| Option              | Description               | Default   |
| ------------------- | ------------------------- | --------- |
| `-g, --group GROUP` | Filter hosts by group tag | All hosts |

## Examples

```bash
HOST=$(auberge select host)
HOST=$(auberge select host --group production)
auberge ansible run --host "$(auberge select host)"
```

Exit 0 on selection, 1 on cancel (Esc) or error.
