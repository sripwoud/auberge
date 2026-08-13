# auberge select host

Interactively select a host and print its name to stdout. Alias: `auberge se h`.

```bash
auberge select host [OPTIONS]
```

Every command taking `--host` opens this same picker when the flag is omitted, so wrapping a single one in `$(...)` buys nothing. Reach for it to pick **once** and reuse the name — across several commands, or in a tool outside auberge.

The picker draws on stderr, so it stays visible when stdout is captured or piped.

## Options

| Option              | Description               | Default   |
| ------------------- | ------------------------- | --------- |
| `-g, --group GROUP` | Filter hosts by group tag | All hosts |

## Examples

Pick once, reuse — one prompt instead of one per command:

```bash
HOST=$(auberge select host)
auberge deploy -H "$HOST" paperless
auberge sync hermes -H "$HOST"
```

Feed a tool that isn't auberge:

```bash
ansible-playbook -l "$(auberge select host --group hermes)" site.yml
```

Exit 0 on selection, 1 on cancel (Esc) or error.
