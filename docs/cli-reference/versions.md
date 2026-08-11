# auberge versions

Report the App Version each app's Playbook Meta (`ansible/playbooks/<app>.meta.yml`) declares, per [ADR-0017](https://github.com/sripwoud/auberge/blob/master/meta/adr/0017-app-versions-declared-in-playbook-meta.md). Alias: `auberge v`.

```bash
auberge versions [OPTIONS]
```

DECLARED is the repo's declaration, read from the binary's embedded asset tree — not what any host currently runs. The default run is fully offline.

## Options

| Option                | Description                                                                               | Default |
| --------------------- | ----------------------------------------------------------------------------------------- | ------- |
| `--check-upstream`    | Query each app's datasource for the latest stable release; adds LATEST and STATUS columns | `false` |
| `-o, --output FORMAT` | `human` or `json`                                                                         | `human` |

## Output

Default:

```
┌────────────┬────────────┐
│ APP        │ DECLARED   │
├────────────┼────────────┤
│ actual     │ 26.8.0     │
├────────────┼────────────┤
│ baikal     │ 0.12.1     │
├────────────┼────────────┤
│ …          │ …          │
└────────────┴────────────┘
```

With `--check-upstream`, each app's declared datasource (`npm` registry or GitHub releases) is queried:

```
┌────────────┬────────────┬───────────┬─────────┐
│ APP        │ DECLARED   │ LATEST    │ STATUS  │
├────────────┼────────────┼───────────┼─────────┤
│ actual     │ 26.8.0     │ 26.8.1    │ behind  │
├────────────┼────────────┼───────────┼─────────┤
│ baikal     │ 0.12.1     │ 0.12.1    │ current │
├────────────┼────────────┼───────────┼─────────┤
│ headscale  │ 0.25.1     │ 0.29.3    │ behind  │
├────────────┼────────────┼───────────┼─────────┤
│ …          │ …          │ …         │ …       │
└────────────┴────────────┴───────────┴─────────┘
```

| Status    | Meaning                                                                                                     |
| --------- | ----------------------------------------------------------------------------------------------------------- |
| `current` | Declared >= latest                                                                                          |
| `behind`  | Upstream has a newer stable release                                                                         |
| `unknown` | Either side is not version-shaped (e.g. non-numeric) — mirrors Renovate, which skips values it cannot parse |

Upstream semantics mirror Renovate's `github-releases` behavior: newest non-draft, non-prerelease release whose tag yields a version. A hyphenated suffix (`2.0.1-alpha.1`) counts as prerelease even when GitHub doesn't flag it; `extractVersion` regexes filter monorepo tags.

## Exit codes

Follow the Backup Verdict convention so a cron can branch on drift:

| Code | Meaning                                                                 |
| ---- | ----------------------------------------------------------------------- |
| `0`  | Every app `current` (or no upstream check requested and read succeeded) |
| `1`  | At least one app `behind`                                               |
| `2`  | Operational error                                                       |

`unknown` drift does not fail the gate:

```bash
auberge versions --check-upstream || notify "app versions drifted"
```

## JSON output

```bash
auberge versions --check-upstream -o json
```

```json
{
  "checked_upstream": true,
  "apps": [
    {
      "app": "actual",
      "declared": "26.8.0",
      "latest": "26.8.0",
      "status": "current"
    },
    {
      "app": "baikal",
      "declared": "0.12.1",
      "latest": "0.12.1",
      "status": "current"
    }
  ]
}
```

`latest` and `status` are omitted without `--check-upstream`.

?> Set the `GITHUB_TOKEN` env var to authenticate GitHub API calls with `--check-upstream` and avoid anonymous rate limits.
