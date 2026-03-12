# Check Mode (Dry Run)

Check mode allows you to preview changes before applying them, similar to a dry run.

## Overview

**Purpose:** See what Ansible would change without actually changing anything

**Command:**

```bash
auberge deploy --check
```

**Output:** Shows tasks that would execute and whether they would make changes

## Usage

### Deploy Check Mode (Primary)

```bash
# Interactive: select app(s) and host, then dry-run
auberge deploy --check

# Check a specific app
auberge deploy paperless --check

# Check all apps on a specific host
auberge deploy --all --host auberge --check
```

### Power-User: ansible run --check

For lower-level control (specific playbooks, skip-tags, etc.):

```bash
auberge ansible run --host auberge --playbook playbooks/apps.yml --check

# With skip-tags
auberge ansible run --host auberge --playbook playbooks/apps.yml --check --skip-tags navidrome

# With explicit tags
auberge ansible run --host auberge --check --tags baikal
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
TASK [baikal : Deploy Baikal config] ****
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
TASK [baikal : Install Baikal] ****
ok: [auberge]

TASK [baikal : Deploy configuration] ****
changed: [auberge]

TASK [baikal : Create data directory] ****
ok: [auberge]

TASK [baikal : Deploy systemd service] ****
changed: [auberge]

TASK [baikal : Start Baikal service] ****
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
TASK [baikal : Restart Baikal] ****
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
    name: baikal
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
auberge deploy paperless --check --host auberge

# If output looks good, deploy
auberge deploy paperless --host auberge
```

### After Config Changes

Verify your changes would apply correctly:

```bash
# Edit configuration
vim ansible/roles/baikal/templates/config.j2

# Check what would change
auberge deploy baikal --check --host auberge

# Review output, then apply
auberge deploy baikal --host auberge
```

### Testing New Playbooks

Test playbooks before first execution:

```bash
# Check new playbook (power-user)
auberge ansible run --host staging --playbook playbooks/new-feature.yml --check

# Review output for errors or unexpected changes
# Fix issues, re-check
# Deploy when ready
```

### Auditing Infrastructure

See what's out of sync with desired state:

```bash
auberge deploy --all --check --host auberge
```

**If many "changed" tasks:** System has drifted from configuration

**If few "changed" tasks:** System is consistent with playbooks

## Combining with Verbosity

Add `--verbose` for more detail:

```bash
# Basic check
auberge deploy baikal --check --host auberge

# With verbosity (see file diffs)
auberge deploy baikal --check --host auberge --verbose
```

For multiple verbosity levels, use `ansible run` directly:

```bash
auberge ansible run --host auberge --check --tags baikal -vv
```

**Example output with `-v`:**

```
TASK [baikal : Deploy config] ****
changed: [auberge]
--- before: /etc/baikal/config
+++ after: /tmp/ansible-tmp-*/config.j2
@@ -1,3 +1,3 @@
 [server]
-hosts = 127.0.0.1:5232
+hosts = 0.0.0.0:5232
```

Shows **diff** of what would change.

## Diff Mode

Show file differences for all tasks (power-user):

```bash
auberge ansible run --host auberge --playbook playbooks/apps.yml --check --diff
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
auberge deploy paperless --check --host auberge

# Normal mode still needed to apply changes
auberge deploy paperless --host auberge
```

Check mode is **read-only** - it never modifies the system.

## Best Practices

### Always Check Before Major Changes

```bash
# Before full stack deployment
auberge deploy --all --check --host auberge

# Review output carefully
# Then deploy
auberge deploy --all --host auberge
```

### Combine with Backup

For production systems:

```bash
# 1. Create backup
auberge backup create --host production

# 2. Check what would change
auberge deploy --all --check --host production

# 3. If safe, apply
auberge deploy --all --host production

# 4. Verify, or restore if needed
```

### Use App Targeting for Focused Checks

```bash
# Check only what you're changing
auberge deploy baikal --check --host auberge
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
