# Check Mode (Dry Run)

Check mode allows you to preview changes before applying them, similar to a dry run.

## Overview

**Purpose:** See what Ansible would change without actually changing anything

**Command:**

```bash
auberge ansible check
```

**Output:** Shows tasks that would execute and whether they would make changes

## Usage

### Interactive Mode

```bash
auberge ansible check
```

Prompts for host and playbook selection.

### Non-Interactive Mode

```bash
auberge ansible check --host auberge --playbook playbooks/apps.yml
```

### With Tags

```bash
auberge ansible check --host auberge --tags radicale
```

### Forced (No Confirmation)

```bash
auberge ansible check --host auberge --playbook playbooks/apps.yml --force
```

## Understanding Output

### Status Indicators

Check mode uses the same status indicators as normal mode:

**ok (green):**

```
TASK [caddy : Check Caddy binary exists] ****
ok: [auberge]
```

- Task would run
- No changes would be made
- System already in desired state

**changed (yellow):**

```
TASK [radicale : Deploy Radicale config] ****
changed: [auberge]
```

- Task would make changes
- File/service/package would be modified
- **This is what you're looking for** in check mode

**skipped (cyan):**

```
TASK [bootstrap : Create ansible user] ****
skipping: [auberge]
```

- Task would be skipped
- Conditional not met (e.g., user already exists)

**failed (red):**

```
TASK [fail2ban : Start fail2ban service] ****
failed: [auberge]
```

- Task would fail
- Usually due to missing dependencies
- May be false positive in check mode (see limitations)

### Example Output

```
TASK [radicale : Install Radicale] ****
ok: [auberge]

TASK [radicale : Deploy configuration] ****
changed: [auberge]

TASK [radicale : Create data directory] ****
ok: [auberge]

TASK [radicale : Deploy systemd service] ****
changed: [auberge]

TASK [radicale : Start Radicale service] ****
changed: [auberge]

PLAY RECAP ****
auberge  : ok=15  changed=3  unreachable=0  failed=0  skipped=2
```

**Interpretation:**

- 3 tasks would make changes
- 15 tasks would run without changes
- 2 tasks would be skipped
- 0 failures

## What Check Mode Shows

### Files Changes

Shows whether files would be:

- Created
- Modified
- Deleted

**Example:**

```
TASK [caddy : Deploy Caddyfile] ****
changed: [auberge]
```

**Meaning:** Caddyfile content differs from template → would be updated

### Package Changes

Shows whether packages would be:

- Installed
- Updated
- Removed

**Example:**

```
TASK [apt : Install essential packages] ****
ok: [auberge]
```

**Meaning:** All packages already installed at correct version

### Service Changes

Shows whether services would be:

- Started
- Stopped
- Restarted
- Enabled/disabled

**Example:**

```
TASK [radicale : Restart Radicale] ****
changed: [auberge]
```

**Meaning:** Service would be restarted (likely due to config change)

### User/Group Changes

Shows whether users/groups would be:

- Created
- Modified
- Removed

**Example:**

```
TASK [ansible_user : Create ansible user] ****
ok: [auberge]
```

**Meaning:** User already exists with correct settings

## Limitations

Check mode has some limitations due to its non-destructive nature:

### 1. Dependency Failures

Tasks that depend on previous tasks may fail:

**Scenario:**

```yaml
- name: Create directory
  ansible.builtin.file:
    path: /app/data
    state: directory

- name: Copy file to directory
  ansible.builtin.copy:
    src: config.yml
    dest: /app/data/config.yml
```

**Check mode:** First task shows "changed", but directory isn't actually created. Second task may fail because directory doesn't exist.

**Workaround:** Ignore these failures - they wouldn't occur in real execution.

### 2. Conditional Logic

Some tasks check system state and conditionally execute:

```yaml
- name: Reload service
  ansible.builtin.systemd:
    name: radicale
    state: reloaded
  when: config_changed
```

**Check mode:** Condition might evaluate incorrectly if dependent task didn't actually run.

### 3. Command Modules

`ansible.builtin.command` and `ansible.builtin.shell` show "changed" even if they wouldn't change anything:

```yaml
- name: Get system info
  ansible.builtin.command: uname -a
  register: uname_output
```

**Check mode:** Always shows "changed" (because it doesn't actually run the command to check).

**Workaround:** Use `check_mode: no` for read-only commands:

```yaml
- name: Get system info
  ansible.builtin.command: uname -a
  register: uname_output
  check_mode: no # Always run, even in check mode
```

### 4. Handlers

Handlers are triggered but not executed:

```yaml
tasks:
  - name: Deploy config
    ansible.builtin.template:
      src: config.j2
      dest: /etc/app/config.yml
    notify: Restart app

handlers:
  - name: Restart app
    ansible.builtin.systemd:
      name: app
      state: restarted
```

**Check mode:**

- Task shows "changed"
- Handler is notified
- Handler doesn't actually restart service

**Output:**

```
RUNNING HANDLER [Restart app] ****
changed: [auberge]
```

**Meaning:** Handler would run, but service isn't actually restarted in check mode.

## Use Cases

### Before Deployment

Preview changes before deploying:

```bash
# Check what would change
auberge ansible check --host auberge --playbook playbooks/apps.yml

# If output looks good, deploy
auberge ansible run --host auberge --playbook playbooks/apps.yml
```

### After Config Changes

Verify your changes would apply correctly:

```bash
# Edit configuration
vim ansible/roles/radicale/templates/config.j2

# Check what would change
auberge ansible check --host auberge --tags radicale

# Review output, then apply
auberge ansible run --host auberge --tags radicale
```

### Testing New Playbooks

Test playbooks before first execution:

```bash
# Check new playbook
auberge ansible check --host staging --playbook playbooks/new-feature.yml

# Review output for errors or unexpected changes
# Fix issues, re-check
# Deploy when ready
```

### Auditing Infrastructure

See what's out of sync with desired state:

```bash
auberge ansible check --host auberge --playbook playbooks/auberge.yml
```

**If many "changed" tasks:** System has drifted from configuration

**If few "changed" tasks:** System is consistent with playbooks

## Combining with Verbosity

Add `-v` flags for more detail:

```bash
# Basic check
auberge ansible check --host auberge --tags radicale

# With verbosity (see file diffs)
auberge ansible check --host auberge --tags radicale -v

# More verbosity (see all task parameters)
auberge ansible check --host auberge --tags radicale -vv
```

**Example output with `-v`:**

```
TASK [radicale : Deploy config] ****
changed: [auberge]
--- before: /etc/radicale/config
+++ after: /tmp/ansible-tmp-*/config.j2
@@ -1,3 +1,3 @@
 [server]
-hosts = 127.0.0.1:5232
+hosts = 0.0.0.0:5232
```

Shows **diff** of what would change.

## Diff Mode

Show file differences for all tasks:

```bash
auberge ansible check --host auberge --playbook playbooks/apps.yml --diff
```

**Output:**

```
TASK [caddy : Deploy Caddyfile] ****
--- before: /etc/caddy/Caddyfile
+++ after: /tmp/ansible-tmp-1234/Caddyfile.j2
@@ -10,6 +10,10 @@
 rss.example.com {
     reverse_proxy localhost:8000
 }
+
+music.example.com {
+    reverse_proxy localhost:4533
+}
```

**Benefit:** See exact changes before applying.

## Real Execution After Check

Check mode and normal mode are independent:

```bash
# Check mode does NOT affect system
auberge ansible check --host auberge --playbook playbooks/apps.yml

# Normal mode still needed to apply changes
auberge ansible run --host auberge --playbook playbooks/apps.yml
```

Check mode is **read-only** - it never modifies the system.

## Best Practices

### Always Check Before Major Changes

```bash
# Before full stack deployment
auberge ansible check --host auberge --playbook playbooks/auberge.yml

# Review output carefully
# Then deploy
auberge ansible run --host auberge --playbook playbooks/auberge.yml --skip-tags bootstrap
```

### Combine with Backup

For production systems:

```bash
# 1. Create backup
auberge backup create --host production

# 2. Check what would change
auberge ansible check --host production --playbook playbooks/apps.yml

# 3. If safe, apply
auberge ansible run --host production --playbook playbooks/apps.yml

# 4. Verify, or restore if needed
```

### Use Tags for Focused Checks

```bash
# Check only what you're changing
auberge ansible check --host auberge --tags radicale

# Not necessary to check everything
```

### Ignore Expected Failures

Some failures in check mode are normal:

- Missing directories that would be created
- Services that would be installed
- Dependencies not yet deployed

**Focus on:** Tasks that should already exist but show "changed" or "failed" unexpectedly.

## Related Pages

- [Running Playbooks](deployment/running-playbooks.md) - Actual execution
- [Tags](deployment/tags.md) - Selective execution
- [Playbooks](core-concepts/playbooks.md) - Playbook structure
