# auberge config

Manage user configuration

## Usage

```bash
auberge config <COMMAND>
# Alias: auberge c
```

## Subcommands

| Command | Alias | Description                                                |
| ------- | ----- | ---------------------------------------------------------- |
| init    | i     | Print a config.toml scaffold derived from the Key Registry |
| set     | s     | Set a config value                                         |
| get     | g     | Get a config value                                         |
| list    | l     | List all config keys (sensitive values redacted)           |
| remove  | rm    | Remove a key from config                                   |
| edit    | e     | Open config in $EDITOR                                     |
| path    | p     | Print config file path                                     |

### `init` flags

| Flag                  | Description                                                              |
| --------------------- | ------------------------------------------------------------------------ |
| `--playbooks <a,b,c>` | Emit only keys in the union of the named Playbook Metas' `required_keys` |
| `--output, -o <path>` | Write the scaffold to a file (refuses to overwrite without `--force`)    |
| `--force, -f`         | Overwrite the output file if it exists                                   |

## Interactive Selection

When `key` is omitted from `set`, `get`, or `remove`, an interactive fuzzy-search selector appears (requires a TTY). For `set`, the value is also prompted interactively if omitted.

In non-interactive mode (pipes, scripts), a key argument is required for `get` and `remove`, and both key and value are required for `set`.

## Examples

```bash
# Print full scaffold to stdout
auberge config init

# Generate config.toml at the XDG config path
auberge config init --output "$(auberge config path)"

# Scope the scaffold to specific playbooks
auberge config init --playbooks paperless,navidrome

# Overwrite an existing file
auberge config init --output config.toml --force

# Set a value directly
auberge c s admin_user_name myuser

# Set interactively (select key, then type value)
auberge c s

# List all keys
auberge c l

# Get a specific value
auberge c g domain

# Get interactively (select key from list)
auberge c g

# Remove interactively
auberge c rm

# Edit in your editor
auberge c e
```

Use `auberge config --help` for full options.
