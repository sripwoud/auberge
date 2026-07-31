# auberge bichon rescan

## Synopsis

```bash
auberge bichon rescan [--host <HOST>] [--account <EMAIL>] [OPTIONS]
```

## Description

Re-archives mail the hourly archive run can never reach.

The archive cursor is a **message-date watermark**: each run asks Bichon only for envelopes dated after `cursor − overlap` (24h). Mail that enters a Synced Folder with an older `Date:` sits below the watermark forever — present in Bichon's store, absent from the Email Archive, unreported. That happens when mail is:

- moved between folders upstream (IMAP `MOVE` preserves `INTERNALDATE`, RFC 6851 — no date the filter reads changes),
- bulk-imported from an old mailbox,
- delivered with a backdated or wrong `Date:` header,
- brought in by `reconcile-folders --apply` adding a folder whose backlog is old.

`rescan` resets the per-account cursor to `0` as the `bichon` user (the file is `0600 bichon:bichon`; a root write would change its owner) and starts `bichon-archive.service`, waiting for the full pass to complete. The archive's skip guard keeps the pass idempotent: already-archived bodies are skipped, only missing ones download. A second consecutive run reports `processed=0`.

The service processes every account each run; `--account` chooses whose cursors are reset. The report covers all accounts, flagging which were reset.

Refuses to run when:

- **any sidecar lacks `message_id`** — a full pass keyed on regenerating envelope ids would multiply duplicate bodies (#389). Remediation: deploy the current bichon role and let the hourly run backfill sidecars, then retry.
- **an archive run is in progress** — a reset mid-run would be overwritten by that run's own cursor write.

## Options

| Option              | Description                                                                       | Default  |
| ------------------- | --------------------------------------------------------------------------------- | -------- |
| -H, --host HOST     | Target host running Bichon (prompted on a TTY)                                    | prompted |
| --account EMAIL     | Only reset one account's cursor (prompted on a TTY, `All accounts` offered first) | All      |
| -o, --output FORMAT | Output format (`human`, `json`)                                                   | `human`  |

Off a TTY, `--host` is required and the error names it; `--account` omitted means all accounts.

## Exit codes

| Code | Meaning                                                |
| ---- | ------------------------------------------------------ |
| 0    | run completed, no failures                             |
| 1    | the run counted failures                               |
| 2    | usage error, unreachable host, or refused precondition |

## Access

Needs both the Bichon API (account list — same base URL resolution and `bichon_api_token` as [reconcile-folders](cli-reference/bichon/reconcile-folders.md)) and SSH to the host (cursor reset, service start, journal read).

## Examples

```bash
# Full pass, all accounts
auberge bichon rescan --host auberge

# One account, structured output
auberge bichon rescan --host auberge --account me@example.com --output json

# Scriptable: exit code carries the verdict
auberge bichon rescan --host auberge --output json || echo "rescan not clean: $?"
```

## When to run

- After `reconcile-folders --apply` adds folders (the command prints this hint itself).
- After importing an old mailbox or reorganising folders upstream.
- When `bichon-expunge.sh`'s coverage gate reports archive < IMAP for a window the hourly run should have covered.
