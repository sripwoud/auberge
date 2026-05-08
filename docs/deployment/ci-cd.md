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
          mkdir -p ~/.ssh/identities
          echo "${{ secrets.SSH_PRIVATE_KEY }}" > ~/.ssh/identities/ansible_production
          chmod 600 ~/.ssh/identities/ansible_production
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
