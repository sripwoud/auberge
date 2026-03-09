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

## Examples

```bash
# Initialize config
auberge config init

# Set a value
auberge c s admin_user_name myuser

# List all keys
auberge c l

# Get a specific value
auberge c g domain

# Edit in your editor
auberge c e
```

Use `auberge config --help` for full options.
