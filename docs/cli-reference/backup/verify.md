# auberge backup verify

Assert that the latest offsite restic snapshot is fresh and holds an app's backup. Read-only — it never writes to the repository. Alias: `auberge b v`.

```bash
auberge backup verify [OPTIONS]
```

## Options

| Option                | Description                                    | Default                                       |
| --------------------- | ---------------------------------------------- | --------------------------------------------- |
| `-H, --host HOST`     | Host whose snapshots to check                  | The sole configured host; required if several |
| `-a, --app APP`       | Also assert this app is in the latest snapshot | No app check                                  |
| `--max-age DURATION`  | Freshness threshold, `<number><s\|m\|h\|d>`    | `24h`                                         |
| `-o, --output FORMAT` | `human` or `json`                              | `human`                                       |

Verify never prompts, so it is safe in scripts and timers.

## Checks

Fail-fast, in order:

1. The restic repository answers `restic snapshots --json`.
2. At least one snapshot exists for the host.
3. The latest snapshot contains the app's directory — only with `--app`.
4. The latest snapshot is younger than `--max-age`.

```bash
$ auberge backup verify --app bichon
✓ repository reachable
✓ latest snapshot for myserver: a1b2c3d4 (2026-07-29T03:00Z, 6h ago)
✓ contains bichon (…/myserver/2026-07-29_03-00-00/bichon)
✓ younger than 24h
verified
```

The checklist is data on stdout; remediation for a failed check goes to stderr.

## Exit codes

| Code | Meaning                                                                                               |
| ---- | ----------------------------------------------------------------------------------------------------- |
| `0`  | Verified — every check passed                                                                         |
| `1`  | A check failed — no snapshot for the host, app missing, or snapshot older than `--max-age`            |
| `2`  | Operational error — restic not installed, repository unreachable, config keys or `--max-age` unusable |

Gate a destructive step on it:

```bash
auberge backup verify --app bichon || exit 1
```

## Examples

```bash
auberge backup verify
auberge backup verify --app bichon
auberge backup verify --host myserver --max-age 36h
auberge backup verify --app bichon --output json
```

## Prerequisites

Same as [backup push](cli-reference/backup/push.md) — requires `restic_repository` and `restic_password` config values.

<details>
<summary>JSON output schema</summary>

```json
{
  "verified": false,
  "status": "check_failed",
  "host": "myserver",
  "app": "bichon",
  "max_age": "24h",
  "snapshot": {
    "id": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2",
    "short_id": "a1b2c3d4",
    "time": "2026-07-29T03:00:00Z",
    "age_seconds": 21600
  },
  "checks": [
    {
      "name": "repository_reachable",
      "passed": true,
      "message": "repository reachable",
      "remediation": null
    },
    {
      "name": "contains_app",
      "passed": false,
      "message": "contains bichon",
      "remediation": "run: auberge backup sync --host myserver --apps bichon"
    }
  ]
}
```

| Field                  | Type           | Description                                                             |
| ---------------------- | -------------- | ----------------------------------------------------------------------- |
| `verified`             | boolean        | `true` only when every check passed                                     |
| `status`               | string         | `verified`, `check_failed`, or `operational_error`                      |
| `host`                 | string         | Host the snapshots were filtered by                                     |
| `app`                  | string \| null | App asserted with `--app`; `null` when omitted                          |
| `max_age`              | string         | Threshold as passed on the command line                                 |
| `snapshot`             | object \| null | Resolved snapshot; `null` when none was found                           |
| `snapshot.short_id`    | string         | First 8 characters of `snapshot.id`, as restic displays it              |
| `snapshot.age_seconds` | number         | Snapshot age at the time of the check                                   |
| `checks`               | array          | Checks that ran, in order — fail-fast, so it stops at the first failure |
| `checks[].name`        | string         | `repository_reachable`, `snapshot_exists`, `contains_app`, or `fresh`   |
| `checks[].remediation` | string \| null | Command to fix a failed check; `null` when it passed                    |

JSON goes to stdout; human-format chrome goes to stderr.

</details>

?> A snapshot belongs to a host if [backup push](cli-reference/backup/push.md) tagged it with the host name — the same tag [backup prune](cli-reference/backup/prune.md) groups retention by, so both commands agree on which snapshots are a host's. Snapshots pushed before tagging landed carry no tags and are matched by their `…/backups/<host>/<timestamp>` path instead, so an existing repository verifies without a re-push.
