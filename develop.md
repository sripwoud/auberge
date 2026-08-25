# Development Setup

## Prerequisites

- Rust toolchain (1.89+, for `File::lock_shared` — see ADR-0034)
- Ansible (for playbook development)
- mise (for environment management)
- age (for encrypting secrets)

## Bootstrap

Run the setup script to install required development tools:

```bash
./setup
```

This installs:

- mise (environment and tool version manager)
- Rust toolchain
- Ansible and ansible-lint
- Other dev dependencies (dprint, dasel, pkl, etc.)

## Configuration

All configuration is managed via `config.toml`. Generate one from the Key Registry:

```bash
auberge config init --output "$(auberge config path)"
```

`auberge config init` prints a TOML scaffold to stdout by default, derived
from `ansible/keys.yml` (the Key Registry). Pass `--playbooks <a,b,c>` to
emit only the keys required by specific playbooks, or `--output <path>` to
write to a file (refuses to overwrite without `--force`).

Key values to set:

```bash
auberge config set admin_user_name yourname
auberge config set admin_user_email you@example.com
auberge config set domain example.com
auberge config set cloudflare_dns_api_token your-token
auberge config set baikal_admin_password your-password
auberge config set gokapi_admin_user admin
auberge config set gokapi_admin_password your-password
auberge config set tailscale_authkey your-authkey
auberge config set ssh_port 22022
```

Run `auberge config init` to inspect the full list of known keys with their
documentation strings.

## Host Management

Hosts can be managed in two ways:

### Option 1: XDG Config (Recommended for end users)

For users installing via `cargo install`, hosts are managed in `~/.config/auberge/hosts.toml`:

```bash
# Add a host interactively
auberge host add my-vps

# Or non-interactively
auberge host add my-vps 203.0.113.10 --user admin --port 22

# List hosts
auberge host list

# Show host details
auberge host show my-vps

# Edit host
auberge host edit my-vps

# Remove host
auberge host remove my-vps
```

The `hosts.toml` format:

```toml
[[hosts]]
name = "auberge"
address = "203.0.113.10"
user = "sripwoud"
port = 22
tags = ["production"]
description = "Main VPS"
```

### Option 2: Ansible Inventory (Recommended for developers)

For development, keep using `ansible/inventory.yml` in the repository:

```yaml
all:
  children:
    vps:
      hosts:
        auberge:
          ansible_host: "{{ lookup('env', 'AUBERGE_HOST') }}"
          ansible_port: 22
          bootstrap_user: root
```

### Priority Order

The CLI checks hosts in this order:

1. `~/.config/auberge/hosts.toml` (if exists and not empty)
2. `ansible/inventory.yml` (fallback for developers)

## API Keys and Tokens

### Cloudflare API Token

Required for DNS-01 ACME challenges via Lego certificate automation:

1. Log into [Cloudflare Dashboard](https://dash.cloudflare.com)
2. Navigate to: My Profile → API Tokens → Create Token
3. Use the "Edit zone DNS" template
4. Configure permissions:
   - Zone → DNS → Edit
   - Zone → Zone → Read
5. Set zone resources to your domain
6. Copy the token and add to config:
   ```bash
   auberge config set cloudflare_dns_api_token your-token
   ```

**Note**: IP whitelisting is optional (all IPs are allowed by default)

### Tailscale

Required for VPN mesh networking:

1. Generate auth key at [Tailscale Admin Console](https://login.tailscale.com/admin/settings/keys)
2. Set reusable and ephemeral flags as needed
3. Store the key:
   ```bash
   auberge config set tailscale_authkey your-authkey
   ```

## Building

```bash
mise run build
# or
cargo build --release
```

Binary output: `target/release/auberge`

## Ansible Assets

The `ansible/` tree is embedded in the binary at compile time. At runtime it
resolves in this order:

1. `AUBERGE_DEV` set, and `./ansible/{playbooks,roles}` exist — run straight
   from the working tree, no extraction.
2. Otherwise — extract the embedded copy to
   `~/.local/share/auberge/ansible/<version>+<content-hash>`, where the hash
   covers the path and bytes of every embedded file.

The fingerprint is the directory name, so a tree is immutable: editing a
playbook, role or template changes the hash and the next run extracts a
_sibling_, never over the tree a concurrent run is still reading from (#628,
ADR-0034). `build.rs` declares `ansible/` a build input, so adding or deleting a
file rebuilds too.

Deploying an unreleased ansible change therefore needs no env var — `cargo
build` is enough. `AUBERGE_DEV=1` still helps when iterating without
recompiling, since it reads the working tree on every run; it only works from
the repository root, as the path is relative.

Two locks keep concurrent invocations off each other:

| lock                          | mode      | held for                                  |
| ----------------------------- | --------- | ----------------------------------------- |
| `ansible/.lock`               | exclusive | one process's extract-then-sweep window   |
| `ansible/<fingerprint>/.lock` | shared    | the lifetime of one `AnsibleAssets` value |

Extraction writes a `.staging*` directory and `rename`s it into place, so a
half-written tree is never visible. The sweep then removes siblings it can lock
exclusively — an abandoned tree, a crashed run's staging directory, the pre-#628
flat layout — and leaves alone anything still locked or unrecognised.

The `ansible-galaxy` cache is shared across trees at
`ansible/collections/<requirements-hash>`, keyed on `requirements.yml` so a
changed requirement lands beside the old cache instead of replacing it.

## Testing

```bash
mise run test
```

`test` depends on `test-*`, so it fans out to every suite:

| Task          | Suite                                             |
| ------------- | ------------------------------------------------- |
| `test-rust`   | `cargo nextest run` — unit tests and `tests/*.rs` |
| `test-shell`  | the `tests/*.test.sh` harnesses                   |
| `test-python` | the baikal suites in `ansible/roles/baikal/files` |

`test-python` runs under `uv`, pinned to the Host's Python minor and
provisioning the same unpinned packages the baikal role installs into the busy
feed venv, so the tests resolve what the Host resolves.
`tests/test_pipeline.rs` fails the build if that list stops matching the
role's, if a `test_*.py` lands somewhere the task does not collect from, or if
`.github/workflows/master.yml` stops running the identical command — mise-action
overwrites `mise.toml` in CI, so the workflow repeats the command rather than
calling the task (ADR-0039). The same file pins `test-shell`: every `*.test.sh`
in the repository must appear in the task, and the workflow's `check` job must
run exactly that set (#649).

## Linting

```bash
mise run lint
# or separately
cargo clippy
ansible-lint
```

## VPS Provider Firewall

Before running bootstrap playbook, configure your VPS provider's firewall to allow your custom SSH port:

1. Check your SSH port: `auberge config get ssh_port`
2. Log into your VPS provider control panel (IONOS, DigitalOcean, Hetzner, etc.)
3. Add firewall rule to allow TCP traffic on your `ssh_port`
4. Save the rule

This prevents lockout when Ansible changes SSH from port 22 to your custom port.
