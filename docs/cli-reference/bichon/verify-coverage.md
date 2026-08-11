# auberge bichon verify-coverage

## Synopsis

```bash
auberge bichon verify-coverage --host <HOST> --account <EMAIL> --folder <NAME> --before <YYYY-MM-DD> [OPTIONS]
```

## Description

Proves, message by message, that the Email Archive covers one folder's mail older than a cutoff — by identity, not by count.

Counts are not identities: the archive is append-only, so after a folder's first expunge it permanently out-counts the mailbox for any window, and a count-based check passes on surplus while specific messages have no archived counterpart. The one class of mail that produces such messages is exactly the one the archive's hourly run cannot see: mail entering a Synced Folder below the [Archive Cursor](cli-reference/bichon/rescan.md) (moved, bulk-imported, or backdated — a message-date watermark misses all three).

Bichon's Internal Store syncs by UID and has no date blindspot, so it knows that mail. The check compares the store's in-window envelopes for the folder against the sidecars' canonical `message_id` set (ADR-0013) and names every store message the archive cannot vouch for.

A message with no `Message-ID` header is unverifiable by identity — the store holds a synthetic id that regenerates on re-import, the sidecar a `sha256:` body hash, and neither side can compute the other's. Those are bounded by count instead: more synthetic store messages in the window than `sha256:`-keyed sidecars in scope is a gap, and both counts appear in the report.

Refuses to answer (exit 2) when a scoped sidecar lacks `message_id` — that is an unknown, not a zero — naming the file; one `bichon-archive.service` run backfills it.

What this does **not** prove: that the live Upstream Mailbox holds nothing the store has never ingested. That residual needs per-message identity from IMAP, which himalaya cannot provide cheaply (#400); `examples/bichon-expunge.sh` keeps its count comparison against the live mailbox alongside this verdict.

## Options

| Option              | Description                                           | Default                   |
| ------------------- | ----------------------------------------------------- | ------------------------- |
| -H, --host HOST     | Target host running Bichon                            | required                  |
| --account EMAIL     | Account email; also names the Email Archive directory | required                  |
| --folder NAME       | Folder whose coverage to verify (exact, case matters) | required                  |
| --before YYYY-MM-DD | Verify messages dated strictly before this UTC date   | required                  |
| --archive-path PATH | Email Archive root on the host                        | `/var/lib/bichon-archive` |
| -o, --output FORMAT | Output format (`human`, `json`)                       | `human`                   |

## Exit codes

| Code | Meaning                                                     |
| ---- | ----------------------------------------------------------- |
| 0    | covered — every store message in the window is archived     |
| 1    | gap — the JSON/human output names the missing messages      |
| 2    | usage error, unreachable host or API, or an unkeyed sidecar |

## Output

```json
{
  "host": "auberge",
  "account": "me@example.com",
  "folder": "INBOX",
  "before": "2026-05-13",
  "status": "covered",
  "store_messages": 388,
  "matched": 388,
  "missing": [],
  "unverifiable": { "store_synthetic": 0, "archive_sha256": 0 }
}
```

`missing` entries carry the canonical `message_id` (what the sidecar would record), the message date, and the store uid.

## Access

Needs both the Bichon API (account lookup and envelope search — same base URL resolution and `bichon_api_token` as [reconcile-folders](cli-reference/bichon/reconcile-folders.md)) and SSH to the host (the sidecar walk runs under `sudo`).

## Examples

```bash
# One folder, human verdict
auberge bichon verify-coverage --host auberge --account me@example.com --folder INBOX --before 2026-05-13

# Scriptable: gate 3 of bichon-expunge.sh consumes this
auberge bichon verify-coverage --host auberge --account me@example.com \
  --folder INBOX --before 2026-05-13 --output json
```

## When a gap is found

Run [`auberge bichon rescan --host <HOST> --account <EMAIL>`](cli-reference/bichon/rescan.md): a full pass archives everything the cursor skipped, and a re-run of this command then reports `covered`.
