# Implementation Plan: Add --force/-f Flag for CI/CD Automation

**Issue:** #16
**Objective:** Add `--force/-f` flag to skip interactive confirmation prompts for CI/CD and automation scenarios

## Design Decisions

### 1. Flag Behavior (clig.dev compliant)

- Use `-f`/`--force` following clig.dev convention for bypassing confirmations
- Warnings still display (security-critical information remains visible)
- Only interactive y/N prompts are skipped
- Exit code 0 when --force used (assumes user completed prerequisites)

### 2. Bootstrap IP Address Handling

- Add optional `--ip` flag to Bootstrap command
- When `--force` is used, `--ip` becomes required
- IP validation: Basic IPv4/IPv6 format check (fail fast on invalid format)
- Keep interactive prompt when --force is not used

### 3. Command Coverage

Apply --force flag to:

- `ansible run` - Skip 3 confirmation prompts (firewall, cloudflare, port 853)
- `ansible check` - Skip same prompts (calls run_ansible_run internally)
- `ansible bootstrap` - Skip bootstrap firewall prompt

### 4. Security Considerations

- Warnings remain visible even with --force
- User assumes responsibility for prerequisites when using --force
- No reduction in information provided, only automation of "yes" response
- Document security implications in help text and README

## File Layout

### Files to Modify

- `src/commands/ansible.rs` - Add flags to command definitions, update prompt logic
- `Cargo.toml` - Ensure `regex` crate available for IP validation

### Files to Create

- None (inline implementation)

## Class/Method/Function Structure

### Command Struct Updates

```rust
enum AnsibleCommands {
    Run {
        // existing fields...
        force: bool, // NEW
    },
    Check {
        // existing fields...
        force: bool, // NEW
    },
    Bootstrap {
        host: String,
        port: u16,
        ip: Option<String>, // NEW
        force: bool,        // NEW
    },
}
```

### Helper Functions

1. `validate_ip(ip: &str) -> Result<()>`
   - Validates IPv4/IPv6 format using regex
   - Returns descriptive error on invalid format

2. `prompt_for_ip(host_name: &str) -> Result<String>`
   - Extracted from existing inline prompt code
   - Handles interactive IP input

3. Updated function signatures:
   - `run_ansible_run(..., force: bool)` - NEW parameter
   - `run_ansible_check(..., force: bool)` - NEW parameter
   - `run_ansible_bootstrap(..., ip: Option<String>, force: bool)` - NEW parameters

## Function Pseudocode

### validate_ip(ip: &str) -> Result<()>

```
1. Define IPv4 regex: ^(\d{1,3}\.){3}\d{1,3}$
2. Define IPv6 regex: simplified pattern for hex segments
3. Check if IP matches either pattern
4. If match:
   - For IPv4: validate each octet is 0-255
   - Return Ok(())
5. If no match:
   - Return Err with message "Invalid IP format: {ip}"
```

### prompt_for_ip(host_name: &str) -> Result<String>

```
1. Print "Enter IP address for {host_name}: "
2. Flush stdout
3. Read line from stdin
4. Return trimmed input
5. Wrap errors with "Failed to read IP address"
```

### run_ansible_bootstrap(..., ip: Option<String>, force: bool)

```
1. Get host from inventory
2. Check bootstrap playbook exists
3. Determine IP address:
   - If ip.is_some():
     * Validate IP format (fail if invalid)
     * Use provided IP
   - Else if force:
     * Return error "--ip is required when using --force"
   - Else:
     * Call prompt_for_ip() (interactive)
4. Show bootstrap firewall warning if is_fresh_bootstrap
5. If !force and is_fresh_bootstrap:
   - Show interactive y/N prompt
   - Exit if user doesn't confirm
6. Else if force and is_fresh_bootstrap:
   - Show "🤖 Skipping confirmation (--force enabled)"
7. Run bootstrap playbook
8. Return success/failure
```

### run_ansible_run(..., force: bool)

```
1. Select host and playbook (existing logic)
2. Determine playbook type (bootstrap/apps/auberge)
3. For each applicable warning:
   - Show warning text (always)
   - If !force:
     * Show y/N prompt
     * Exit if user doesn't confirm
   - Else:
     * Show "🤖 Skipping confirmation (--force enabled)"
4. Run playbook (existing logic)
5. Return success/failure
```

## TDD Plan

### Unit Tests

Location: `src/commands/ansible.rs` (inline tests module)

1. **test_validate_ip_valid_ipv4**
   - Input: "192.168.1.1", "10.0.0.1", "172.16.0.1"
   - Expected: Ok(())

2. **test_validate_ip_valid_ipv6**
   - Input: "::1", "2001:db8::1", "fe80::1"
   - Expected: Ok(())

3. **test_validate_ip_invalid_format**
   - Input: "999.999.999.999", "not-an-ip", "192.168.1", "192.168.1.1.1"
   - Expected: Err with descriptive message

4. **test_validate_ip_edge_cases**
   - Input: "", " ", "localhost", "192.168.1.1 "
   - Expected: Err for empty/spaces/localhost, Ok for trimmed input

### Integration Tests

Run manually in test environment:

1. **Test: Run with --force (all prompts skipped)**
   ```bash
   ./auberge ansible run --host test --playbook playbooks/apps.yml --force
   ```
   Expected: Warnings display, no prompts, proceeds to execution

2. **Test: Bootstrap with --force and --ip**
   ```bash
   ./auberge ansible bootstrap test --ip 192.168.1.100 --force
   ```
   Expected: Warning displays, no prompts, uses provided IP

3. **Test: Bootstrap with --force without --ip (error)**
   ```bash
   ./auberge ansible bootstrap test --force
   ```
   Expected: Error "--ip is required when using --force"

4. **Test: Bootstrap with invalid --ip**
   ```bash
   ./auberge ansible bootstrap test --ip invalid-ip --force
   ```
   Expected: Error "Invalid IP format: invalid-ip"

5. **Test: Interactive mode unchanged**
   ```bash
   ./auberge ansible run --host test --playbook playbooks/bootstrap.yml
   ```
   Expected: Warnings + prompts displayed, waits for y/N input

6. **Test: Check with --force**
   ```bash
   ./auberge ansible check --host test --playbook playbooks/apps.yml --force
   ```
   Expected: Warnings display, no prompts, runs in check mode

## Commit Plan

### Phase 1: Foundation

#### Commit 1: `feat(cli): add force flag to ansible commands`

- Phase: Foundation
- Layers: backend
- Changes:
  - Add `force: bool` field to Run, Check, Bootstrap structs with clap attribute
  - Add help text: "Skip confirmation prompts (for CI/CD automation)"
  - Update function signatures to accept force parameter
  - Pass force through call chain (no behavioral changes yet)
- Tests:
  - Cargo build succeeds
  - `./auberge ansible run --help` shows --force/-f flag
  - `./auberge ansible bootstrap --help` shows --force/-f flag
  - Commands work without --force (existing behavior)

#### Commit 2: `feat(cli): add ip flag to bootstrap command`

- Phase: Foundation
- Layers: backend
- Changes:
  - Add `ip: Option<String>` to Bootstrap struct with clap attribute
  - Extract IP prompt logic into `prompt_for_ip()` helper function
  - Update run_ansible_bootstrap signature to accept ip parameter
  - Use provided IP if present, otherwise call prompt (existing behavior)
- Tests:
  - Cargo build succeeds
  - `./auberge ansible bootstrap --help` shows --ip flag
  - Bootstrap works with --ip flag (skips interactive prompt)
  - Bootstrap works without --ip flag (shows interactive prompt)

### Phase 2: Validation

#### Commit 3: `feat(cli): add ip address format validation`

- Phase: Validation
- Layers: backend
- Changes:
  - Add `regex` dependency to Cargo.toml (if not present)
  - Implement `validate_ip()` function with IPv4/IPv6 regex validation
  - Add unit tests in `#[cfg(test)]` module
- Tests:
  - Unit: test_validate_ip_valid_ipv4 (passes)
  - Unit: test_validate_ip_valid_ipv6 (passes)
  - Unit: test_validate_ip_invalid_format (passes)
  - Unit: test_validate_ip_edge_cases (passes)
  - Cargo test succeeds

#### Commit 4: `feat(cli): enforce ip requirement when force is used`

- Phase: Validation
- Layers: backend
- Changes:
  - Update run_ansible_bootstrap IP handling logic
  - If force && ip.is_none(): return error
  - If ip.is_some(): validate_ip() then use
  - If !force && ip.is_none(): prompt_for_ip()
- Tests:
  - Integration: `bootstrap --force --ip 192.168.1.1` (succeeds)
  - Integration: `bootstrap --force` (fails with clear error)
  - Integration: `bootstrap --ip invalid` (fails validation)
  - Integration: `bootstrap` (interactive prompt works)

### Phase 3: Implementation

#### Commit 5: `feat(cli): skip bootstrap firewall prompt with force flag`

- Phase: Implementation
- Layers: backend
- Changes:
  - Update lines 97-121 in run_ansible_run
  - Wrap prompt in `if !force { ... }` conditional
  - Add else branch showing "🤖 Skipping confirmation (--force enabled)"
  - Warning text still displays in both branches
- Tests:
  - Integration: `run --playbook bootstrap.yml --force` (no prompt)
  - Integration: `run --playbook bootstrap.yml` (shows prompt)
  - Verify warnings display in both cases

#### Commit 6: `feat(cli): skip cloudflare prompt with force flag`

- Phase: Implementation
- Layers: backend
- Changes:
  - Update lines 126-157 in run_ansible_run
  - Apply same conditional pattern as commit 5
  - Consistent messaging for skipped prompts
- Tests:
  - Integration: `run --playbook apps.yml --force` (skips cloudflare prompt)
  - Integration: `run --playbook auberge.yml --force` (skips cloudflare prompt)
  - Integration: `run --playbook apps.yml` (shows cloudflare prompt)
  - Verify warnings display in both cases

#### Commit 7: `feat(cli): skip port 853 firewall prompt with force flag`

- Phase: Implementation
- Layers: backend
- Changes:
  - Update lines 159-182 in run_ansible_run
  - Apply same conditional pattern
  - Complete implementation of all prompt skips
- Tests:
  - Integration: `run --playbook apps.yml --force` (all 3 prompts skipped)
  - Integration: `run --playbook auberge.yml --force` (all prompts skipped)
  - Verify all warnings still display
  - Verify command proceeds without user input

### Phase 4: Documentation

#### Commit 8: `docs(cli): document force flag usage and security notes`

- Phase: Documentation
- Layers: backend
- Changes:
  - Update README with --force flag examples
  - Add CI/CD automation section
  - Document security considerations
  - Add troubleshooting entry for "locked out after bootstrap"
- Tests:
  - Verify examples in README are accurate
  - Verify all commands shown in docs actually work

### Phase 5: Final Review

- Do a PR review of the changes in the branch as if you are another engineer reviewing
- Decide which PR recommendations to follow, apply recommended changes
- Iterate on tests to ensure everything is passing
- Remove any unnecessary comments in the code
