# auberge bichon reconcile-folders

## Synopsis

```bash
auberge bichon reconcile-folders --host <HOST> [OPTIONS]
```

## Description

Reconciles Bichon account `sync_folders` from the live IMAP folder list.

Policy:

- Include all folders by default.
- Exclude folders with SPECIAL-USE `\Junk` and `\Trash`.
- Fallback for legacy IMAP servers without SPECIAL-USE: exclude common leaf names (`spam`, `junk`, `junk mail`, `trash`, `deleted items`, `deleted messages`, `bin`).
- Apply per-account additive overrides from `config.toml`:

```toml
[bichon.account_overrides."me@example.com"]
extra_excluded_folders = ["Newsletters", "Receipts/2019"]
```

Default is dry-run (read-only).

## Options

| Option              | Description                                    | Default |
| ------------------- | ---------------------------------------------- | ------- |
| -H, --host HOST     | Target host running Bichon                     | -       |
| --apply             | Commit `sync_folders` updates in Bichon        | false   |
| --account EMAIL     | Reconcile only one account email               | All     |
| -o, --output FORMAT | Output format (`human`, `json`)                | `human` |

## Examples

```bash
# Dry-run all accounts
auberge bichon reconcile-folders --host auberge

# Apply changes
auberge bichon reconcile-folders --host auberge --apply

# Single account only
auberge bichon reconcile-folders --host auberge --account me@example.com --apply

# Structured output
auberge bichon reconcile-folders --host auberge --output json
```
