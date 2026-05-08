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
- Fallback for legacy IMAP servers without SPECIAL-USE: exclude common leaf names (`spam`, `junk`, `junk mail`, `junk email`, `trash`, `deleted items`, `deleted messages`, `bin`). The fallback is case-insensitive and matches on the leaf segment (`Archive/Spam` matches; `Spamtastic` does not).
- Apply per-account additive overrides from `config.toml`:

```toml
[bichon.account_overrides."me@example.com"]
extra_excluded_folders = ["Newsletters", "Receipts/2019"]
```

?> `extra_excluded_folders` matches the **full mailbox path, case-sensitively**. Use the exact name returned by Bichon's `list-mailboxes` API (e.g. `"Receipts/2019"`, not `"receipts/2019"`). Unlike the SPECIAL-USE / leaf-name fallback, this is intentionally precise so operators can target individual sub-folders without false positives.

Default is dry-run (read-only).

## Options

| Option              | Description                             | Default |
| ------------------- | --------------------------------------- | ------- |
| -H, --host HOST     | Target host running Bichon              | -       |
| --apply             | Commit `sync_folders` updates in Bichon | false   |
| --account EMAIL     | Reconcile only one account email        | All     |
| -o, --output FORMAT | Output format (`human`, `json`)         | `human` |

## Bichon API base URL

The base URL for the Bichon API is resolved in this order (first match wins):

1. **Per-host override** in `config.toml`:

   ```toml
   [bichon.hosts.auberge]
   base_url = "https://bichon.auberge.example.com"

   [bichon.hosts.staging]
   base_url = "https://bichon.staging.example.com"
   ```

   Recommended when you operate more than one Bichon instance: each `--host <name>` resolves to its own URL.

2. **Global override** `bichon_base_url`:

   ```toml
   bichon_base_url = "https://bichon.example.com"
   ```

3. **Derived from `domain`** (default):

   ```toml
   domain = "example.com"
   bichon_subdomain = "bichon"   # optional, defaults to "bichon"
   ```

   Yields `https://bichon.example.com`.

If neither a per-host override, a global `bichon_base_url`, nor `domain` is set, the command fails with an explicit error naming the host.

## Required config keys

| Key                                                       | Purpose                                             |
| --------------------------------------------------------- | --------------------------------------------------- |
| `bichon_api_token`                                        | Bearer token for the Bichon API. Required.          |
| `bichon.hosts.<name>.base_url`                            | Per-host base URL override. Optional.               |
| `bichon_base_url`                                         | Global base URL override. Optional.                 |
| `bichon_subdomain`                                        | Subdomain when deriving from `domain`. Optional.    |
| `domain`                                                  | Domain used for default URL derivation.             |
| `bichon.account_overrides.<email>.extra_excluded_folders` | Per-account exclusions (full path, case-sensitive). |

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
