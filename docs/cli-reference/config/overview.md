# auberge config

Manage user configuration

## Usage

```bash
auberge config <COMMAND>
# Alias: auberge c
```

## Subcommands

| Command | Alias | Description                                      |
| ------- | ----- | ------------------------------------------------ |
| init    | i     | Create template config.toml with all keys        |
| set     | s     | Set a config value                               |
| get     | g     | Get a config value                               |
| list    | l     | List all config keys (sensitive values redacted) |
| remove  | rm    | Remove a key from config                         |
| edit    | e     | Open config in $EDITOR                           |
| path    | p     | Print config file path                           |

## Interactive Selection

When `key` is omitted from `set`, `get`, or `remove`, an interactive fuzzy-search selector appears (requires a TTY). For `set`, the value is also prompted interactively if omitted.

In non-interactive mode (pipes, scripts), a key argument is required for `get` and `remove`, and both key and value are required for `set`.

## Examples

```bash
# Initialize config
auberge config init

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
