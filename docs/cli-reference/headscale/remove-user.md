# auberge headscale remove-user

Remove a Headscale user

## Synopsis

```bash
auberge headscale remove-user [NAME] [OPTIONS]
```

## Alias

`auberge hs ru`

## Description

Removes a user from the target Headscale instance. All nodes belonging to the user are also removed from the tailnet. By default, prompts for confirmation before proceeding.

## Options

| Option        | Description                   | Default            |
| ------------- | ----------------------------- | ------------------ |
| `[NAME]`      | Username to remove            | Interactive prompt |
| `--host HOST` | Target host running headscale | Interactive prompt |
| `-y, --yes`   | Skip confirmation prompt      | false              |

## Examples

```bash
# Remove a user (with confirmation prompt)
auberge headscale remove-user --host myserver default

# Remove without confirmation
auberge headscale remove-user --host myserver default --yes
```

## Output Example

```
Remove user 'default'? [y/N]: y
✓ User 'default' removed
```

## Troubleshooting

**User has nodes attached**: All nodes belonging to the user are removed automatically. Verify with `auberge headscale list-nodes` beforehand if you want to review what will be removed.

## Related Commands

- [auberge headscale add-user](add-user.md) - Create a user and generate a pre-auth key
- [auberge headscale list-users](list-users.md) - List registered users
- [auberge headscale list-nodes](list-nodes.md) - List connected nodes

## See Also

- [Headscale](../../applications/networking/headscale.md)
