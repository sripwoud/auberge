# CI/CD Automation

Guide to automating Auberge deployments with CI/CD pipelines.

## Overview

Auberge supports fully automated deployments via the `--force` flag, which skips interactive prompts while still displaying warnings.

**Key features:**

- Skip confirmation prompts
- Explicit host and playbook selection
- Warning messages always displayed
- Exit codes for pipeline integration

## The --force Flag

### What It Does

`--force` (or `-f`) skips interactive confirmations:

```bash
# With --force: no prompts
auberge ansible run --host production --playbook playbooks/apps.yml --force

# Without --force: prompts for confirmation
auberge ansible run --host production --playbook playbooks/apps.yml
# ? Confirm execution? [y/N]:
```

### What It Doesn't Do

**--force does NOT skip warnings:**

```bash
auberge ansible bootstrap production --ip 10.0.0.1 --force
```

**Output:**

```
⚠ WARNING: Ensure provider firewall allows SSH_PORT (2222)
⚠ WARNING: Bootstrap will change SSH port from 22 to 2222
⚠ WARNING: Ensure Cloudflare API token is configured

Proceeding with bootstrap...
```

**Warnings always display** - you're responsible for heeding them.

## GitHub Actions

### Complete Workflow Example

```yaml
name: Deploy to VPS

on:
  push:
    branches: [main]
  workflow_dispatch: # Manual trigger

permissions:
  contents: read

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install Auberge
        run: cargo install auberge

      - name: Set up mise
        uses: jdx/mise-action@v2
        with:
          experimental: true

      - name: Configure secrets
        env:
          AUBERGE_HOST: ${{ secrets.AUBERGE_HOST }}
          SSH_PORT: ${{ secrets.SSH_PORT }}
          ADMIN_USER_NAME: ${{ secrets.ADMIN_USER_NAME }}
          CLOUDFLARE_DNS_API_TOKEN: ${{ secrets.CLOUDFLARE_DNS_API_TOKEN }}
        run: |
          mise set AUBERGE_HOST="$AUBERGE_HOST"
          mise set SSH_PORT="$SSH_PORT"
          mise set ADMIN_USER_NAME="$ADMIN_USER_NAME"
          mise set CLOUDFLARE_DNS_API_TOKEN="$CLOUDFLARE_DNS_API_TOKEN"

      - name: Set up SSH key
        env:
          SSH_PRIVATE_KEY: ${{ secrets.SSH_PRIVATE_KEY }}
        run: |
          mkdir -p ~/.ssh/identities
          echo "$SSH_PRIVATE_KEY" > ~/.ssh/identities/ansible_production
          chmod 600 ~/.ssh/identities/ansible_production

      - name: Deploy applications
        run: |
          auberge ansible run \
            --host production \
            --playbook playbooks/apps.yml \
            --force \
            --skip-tags bootstrap

      - name: Verify deployment
        run: |
          ssh -i ~/.ssh/identities/ansible_production \
              -p ${{ secrets.SSH_PORT }} \
              ansible@${{ secrets.AUBERGE_HOST }} \
              "systemctl is-active baikal freshrss navidrome"
```

### Required GitHub Secrets

Set these in repository settings → Secrets and variables → Actions:

- `AUBERGE_HOST` - VPS IP address
- `SSH_PORT` - Custom SSH port
- `SSH_PRIVATE_KEY` - ansible user's SSH private key (full content)
- `ADMIN_USER_NAME` - Admin username
- `CLOUDFLARE_DNS_API_TOKEN` - Cloudflare API token
- Other app-specific secrets (BAIKAL_ADMIN_PASSWORD, etc.)

## GitLab CI

### .gitlab-ci.yml

```yaml
stages:
  - deploy

deploy_production:
  stage: deploy
  image: rust:latest
  only:
    - main
  before_script:
    - cargo install auberge
    - cargo install mise
    - mkdir -p ~/.ssh/identities
    - echo "$SSH_PRIVATE_KEY" > ~/.ssh/identities/ansible_production
    - chmod 600 ~/.ssh/identities/ansible_production
    - mise set AUBERGE_HOST="$AUBERGE_HOST"
    - mise set SSH_PORT="$SSH_PORT"
    - mise set ADMIN_USER_NAME="$ADMIN_USER_NAME"
    - mise set CLOUDFLARE_DNS_API_TOKEN="$CLOUDFLARE_DNS_API_TOKEN"
  script:
    - auberge ansible run
      --host production
      --playbook playbooks/apps.yml
      --force
      --skip-tags bootstrap
  after_script:
    - ssh -i ~/.ssh/identities/ansible_production
      -p $SSH_PORT
      ansible@$AUBERGE_HOST
      "systemctl status php*-fpm freshrss navidrome"
```

### Required GitLab Variables

Set in Settings → CI/CD → Variables:

- `AUBERGE_HOST`
- `SSH_PORT`
- `SSH_PRIVATE_KEY` (masked, file type)
- `ADMIN_USER_NAME`
- `CLOUDFLARE_DNS_API_TOKEN` (masked)

## Best Practices

### Use Separate Environments

```yaml
# .github/workflows/deploy-staging.yml
- name: Deploy to staging
  run: |
    auberge ansible run \
      --host staging \
      --playbook playbooks/apps.yml \
      --force

# .github/workflows/deploy-production.yml
- name: Deploy to production
  run: |
    auberge ansible run \
      --host production \
      --playbook playbooks/apps.yml \
      --force
```

### Create Backup Before Deploy

```yaml
- name: Create backup
  run: |
    auberge backup create --host production --yes

- name: Deploy
  run: |
    auberge ansible run \
      --host production \
      --playbook playbooks/apps.yml \
      --force

- name: Verify deployment
  run: |
    # Health checks
    curl -f https://cal.example.com || exit 1
```

### Use Check Mode First

```yaml
- name: Check what would change
  run: |
    auberge ansible check \
      --host production \
      --playbook playbooks/apps.yml \
      --force

- name: Deploy if check passed
  run: |
    auberge ansible run \
      --host production \
      --playbook playbooks/apps.yml \
      --force
```

### Tag-Based Deployments

```yaml
- name: Deploy only changed apps
  run: |
    # Detect which apps changed
    CHANGED_APPS=$(git diff --name-only HEAD~1 | grep 'ansible/roles/' | cut -d'/' -f3 | tr '\n' ',' | sed 's/,$//')

    if [ -n "$CHANGED_APPS" ]; then
      auberge ansible run \
        --host production \
        --tags "$CHANGED_APPS" \
        --force
    else
      echo "No app changes detected"
    fi
```

### Rollback on Failure

```yaml
- name: Deploy
  id: deploy
  run: |
    auberge ansible run \
      --host production \
      --playbook playbooks/apps.yml \
      --force

- name: Rollback on failure
  if: failure() && steps.deploy.conclusion == 'failure'
  run: |
    auberge backup restore latest \
      --host production \
      --yes
```

## Security Considerations

### Protect SSH Keys

**Never commit SSH private keys** to version control.

✓ **Good:** Use GitHub Secrets or GitLab Variables
✗ **Bad:** Store in repository

### Use Deploy Keys

Generate dedicated SSH keys for CI/CD:

```bash
# On CI runner or locally
ssh-keygen -t ed25519 -f ~/.ssh/ci_deploy_key -C "ci-deploy@example.com"

# Add public key to VPS
ssh-copy-id -i ~/.ssh/ci_deploy_key.pub ansible@vps-host
```

Add private key to CI secrets:

```bash
cat ~/.ssh/ci_deploy_key  # Copy to GitHub Secrets
```

### Restrict Runner Permissions

Use least-privilege principles:

```yaml
permissions:
  contents: read # Don't grant write unless needed
```

### Audit Deployments

Log all deployments:

```yaml
- name: Log deployment
  run: |
    echo "Deployed by: ${{ github.actor }}"
    echo "Commit: ${{ github.sha }}"
    echo "Branch: ${{ github.ref }}"
    date
```

## Testing in CI

### Ansible Syntax Check

```yaml
- name: Check Ansible syntax
  run: |
    ansible-playbook playbooks/auberge.yml --syntax-check
```

### Lint Playbooks

```yaml
- name: Lint Ansible playbooks
  run: |
    ansible-lint playbooks/
```

### Validate Inventory

```yaml
- name: Validate inventory
  run: |
    ansible-inventory -i ansible/inventory.yml --list
```

## Deployment Strategies

### Blue-Green Deployment

Maintain two VPS hosts:

```yaml
- name: Deploy to blue
  run: |
    auberge ansible run --host blue --playbook playbooks/apps.yml --force

- name: Switch DNS to blue
  run: |
    auberge dns set-all --host blue --force

- name: Drain green
  run: |
    # Wait for connections to drain
    sleep 300

- name: Update green
  run: |
    auberge ansible run --host green --playbook playbooks/apps.yml --force
```

### Canary Deployment

Deploy to subset of hosts first:

```yaml
- name: Deploy to canary
  run: |
    auberge ansible run --host canary --playbook playbooks/apps.yml --force

- name: Smoke test
  run: |
    curl -f https://cal.canary.example.com || exit 1

- name: Deploy to production
  run: |
    auberge ansible run --host production --playbook playbooks/apps.yml --force
```

## Monitoring Deployments

### Send Notifications

```yaml
- name: Notify on success
  if: success()
  run: |
    curl -X POST $WEBHOOK_URL \
      -d "Deployment to production succeeded"

- name: Notify on failure
  if: failure()
  run: |
    curl -X POST $WEBHOOK_URL \
      -d "Deployment to production failed"
```

### Log to External Service

```yaml
- name: Log deployment
  run: |
    curl -X POST https://logging-service.example.com/events \
      -H "Content-Type: application/json" \
      -d '{
        "event": "deployment",
        "status": "success",
        "commit": "${{ github.sha }}",
        "actor": "${{ github.actor }}"
      }'
```

## Example: Minimal Production Pipeline

```yaml
name: Deploy to Production

on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install auberge

      - name: Set up SSH
        env:
          SSH_KEY: ${{ secrets.SSH_PRIVATE_KEY }}
        run: |
          mkdir -p ~/.ssh/identities
          echo "$SSH_KEY" > ~/.ssh/identities/ansible_production
          chmod 600 ~/.ssh/identities/ansible_production

      - name: Deploy
        env:
          AUBERGE_HOST: ${{ secrets.AUBERGE_HOST }}
          SSH_PORT: ${{ secrets.SSH_PORT }}
        run: |
          auberge ansible run \
            --host production \
            --playbook playbooks/apps.yml \
            --force \
            --skip-tags bootstrap
```

## Troubleshooting

### SSH Host Key Verification Failed

Add host key to known_hosts:

```yaml
- name: Add host key
  run: |
    ssh-keyscan -p ${{ secrets.SSH_PORT }} ${{ secrets.AUBERGE_HOST }} >> ~/.ssh/known_hosts
```

### Environment Variables Not Set

Ensure secrets are properly loaded:

```yaml
- name: Debug environment
  run: |
    echo "AUBERGE_HOST: ${AUBERGE_HOST:+SET}"
    echo "SSH_PORT: ${SSH_PORT:+SET}"
```

### Permission Denied (SSH)

Check SSH key permissions:

```yaml
- run: chmod 600 ~/.ssh/identities/ansible_production
```

### Ansible Connection Timeout

Increase timeout in inventory:

```yaml
ansible_ssh_common_args: >-
  -o ConnectTimeout=30
  -o ConnectionAttempts=3
```

## Related Pages

- [Running Playbooks](deployment/running-playbooks.md) - Manual execution
- [Bootstrap](deployment/bootstrap.md) - Initial VPS setup
- [Secrets Management](configuration/secrets.md) - Handling sensitive data
- [SSH Keys](configuration/ssh-keys.md) - SSH configuration
