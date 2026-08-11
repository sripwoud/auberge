# auberge versions

Report every version pin the repo declares, per [ADR-0017](https://github.com/sripwoud/auberge/blob/master/meta/adr/0017-app-versions-declared-in-playbook-meta.md): the App Version from each app's Playbook Meta (`ansible/playbooks/<app>.meta.yml`), and the Tool Versions from `# renovate:` annotations in role defaults (`ansible/roles/<role>/defaults/main.yml`) — `lego`, `uv`, Caddy's plugins. Alias: `auberge v`.

```bash
auberge versions [OPTIONS]
```

DECLARED is the repo's declaration, read from the binary's embedded asset tree — not what any host currently runs. The default run is fully offline.

## Options

| Option                | Description                                                                               | Default |
| --------------------- | ----------------------------------------------------------------------------------------- | ------- |
| `--check-upstream`    | Query each pin's datasource for the latest stable release; adds LATEST and STATUS columns | `false` |
| `-o, --output FORMAT` | `human` or `json`                                                                         | `human` |

## Output

Default — App and Tool Versions stay distinct sections, keeping the ADR-0017 split visible:

```
App Versions
┌────────────┬────────────┐
│ APP        │ DECLARED   │
├────────────┼────────────┤
│ actual     │ 26.8.0     │
├────────────┼────────────┤
│ …          │ …          │
└────────────┴────────────┘

Tool Versions
┌────────┬───────────────────┬──────────┐
│ ROLE   │ TOOL              │ DECLARED │
├────────┼───────────────────┼──────────┤
│ blocky │ lego              │ 5.3.1    │
├────────┼───────────────────┼──────────┤
│ caddy  │ cloudflare_plugin │ v0.2.4   │
├────────┼───────────────────┼──────────┤
│ …      │ …                 │ …        │
└────────┴───────────────────┴──────────┘
```

ROLE/TOOL come from the pinned variable: `blocky_lego_version` reports as role `blocky`, tool `lego`.

With `--check-upstream`, each pin's declared datasource (`npm` registry, GitHub releases, or the Go module proxy for `datasource=go`) is queried:

```
App Versions
┌────────────┬────────────┬───────────┬─────────┐
│ APP        │ DECLARED   │ LATEST    │ STATUS  │
├────────────┼────────────┼───────────┼─────────┤
│ actual     │ 26.8.0     │ 26.8.1    │ behind  │
├────────────┼────────────┼───────────┼─────────┤
│ headscale  │ 0.25.1     │ 0.29.3    │ behind  │
├────────────┼────────────┼───────────┼─────────┤
│ …          │ …          │ …         │ …       │
└────────────┴────────────┴───────────┴─────────┘

Tool Versions
┌────────┬──────┬──────────┬────────┬─────────┐
│ ROLE   │ TOOL │ DECLARED │ LATEST │ STATUS  │
├────────┼──────┼──────────┼────────┼─────────┤
│ blocky │ lego │ 5.3.1    │ 5.3.1  │ current │
├────────┼──────┼──────────┼────────┼─────────┤
│ …      │ …    │ …        │ …      │ …       │
└────────┴──────┴──────────┴────────┴─────────┘
```

| Status    | Meaning                                                                                                     |
| --------- | ----------------------------------------------------------------------------------------------------------- |
| `current` | Declared >= latest                                                                                          |
| `behind`  | Upstream has a newer stable release                                                                         |
| `unknown` | Either side is not version-shaped (e.g. non-numeric) — mirrors Renovate, which skips values it cannot parse |

Upstream semantics mirror Renovate's. `github-releases`: newest non-draft, non-prerelease release whose tag yields a version — a hyphenated suffix (`2.0.1-alpha.1`) counts as prerelease even when GitHub doesn't flag it, and `extractVersion` regexes filter monorepo tags. `go`: newest stable version the module proxy lists under `@v/list`.

## Exit codes

Follow the Backup Verdict convention so a cron can branch on drift:

| Code | Meaning                                                                 |
| ---- | ----------------------------------------------------------------------- |
| `0`  | Every pin `current` (or no upstream check requested and read succeeded) |
| `1`  | At least one App or Tool Version `behind`                               |
| `2`  | Operational error                                                       |

`unknown` drift does not fail the gate:

```bash
auberge versions --check-upstream || notify "version pins drifted"
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
    }
  ],
  "tools": [
    {
      "role": "blocky",
      "tool": "lego",
      "declared": "5.3.1",
      "latest": "5.3.1",
      "status": "current"
    }
  ]
}
```

`latest` and `status` are omitted without `--check-upstream`.

?> Set the `GITHUB_TOKEN` env var to authenticate GitHub API calls with `--check-upstream` and avoid anonymous rate limits.
