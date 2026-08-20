# CI/CD Automation

Auberge supports non-interactive deployments via `--force`, which skips confirmation prompts but always prints warnings.

## Required secrets

| Secret                     | Description                               |
| -------------------------- | ----------------------------------------- |
| `SSH_PRIVATE_KEY`          | ansible user's private key (full content) |
| `SSH_PORT`                 | custom SSH port                           |
| `CLOUDFLARE_DNS_API_TOKEN` | Cloudflare API token                      |
| `ADMIN_USER_NAME`          | admin username                            |

App-specific secrets (e.g. `BAIKAL_ADMIN_PASSWORD`) are also required if used.

## `--force` semantics

`--force` / `-f` skips interactive confirmations. Warnings still print — you are responsible for heeding them.

```bash
auberge ansible run --host production --playbook playbooks/apps.yml --force
```

## Minimal GitHub Actions workflow

```yaml
name: Deploy

on:
  push:
    branches: [main]

permissions:
  contents: read

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install auberge

      - name: Set up SSH key
        run: |
          mkdir -p ~/.ssh/identities/production
          echo "${{ secrets.SSH_PRIVATE_KEY }}" > ~/.ssh/identities/production/ansible
          chmod 600 ~/.ssh/identities/production/ansible
          ssh-keyscan -p "${{ secrets.SSH_PORT }}" "${{ secrets.AUBERGE_HOST }}" >> ~/.ssh/known_hosts

      - name: Configure auberge
        run: |
          auberge config set ssh_port "${{ secrets.SSH_PORT }}"
          auberge config set admin_user_name "${{ secrets.ADMIN_USER_NAME }}"
          auberge config set cloudflare_dns_api_token "${{ secrets.CLOUDFLARE_DNS_API_TOKEN }}"

      - name: Deploy
        run: |
          auberge ansible run \
            --host production \
            --playbook playbooks/apps.yml \
            --force \
            --skip-tags bootstrap
```

?> Verify config is applied with `auberge config list` if a step silently misbehaves.

## Version drift monitoring

`auberge versions --check-upstream` queries npm/GitHub for each app's latest stable release and compares it against the declared App Version. Exit code signals drift, so cron or CI can branch on it:

| Exit code | Meaning                    |
| --------- | -------------------------- |
| `0`       | Every app is current       |
| `1`       | At least one app is behind |
| `2`       | Operational error          |

`-o json` gives machine-readable output. Set `GITHUB_TOKEN` to avoid anonymous GitHub API rate limits.

```yaml
name: Check app versions

on:
  schedule:
    - cron: "0 6 * * 1"

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install auberge
      - run: auberge versions --check-upstream
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

See [`auberge versions`](cli-reference/versions.md) for full command reference.
