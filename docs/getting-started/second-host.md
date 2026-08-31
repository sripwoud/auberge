# Adding a Second Host

First host: see [First Deployment](getting-started/first-deployment.md). A second host reuses the fleet-wide config; you scope what differs.

## Step 1: Add the host

```bash
auberge host add agent-box 203.0.113.20 --user root
auberge ssh keygen --host agent-box
```

The name is permanent identity: bootstrap sets it as the machine's hostname, and its SSH keys live under `~/.ssh/identities/agent-box/`. No name recycling — a rebuilt or repurposed box gets a fresh name.

## Step 2: Scope the config

Every top-level `config.toml` key applies to every host. In particular, `infrastructure.yml` deploys `headscale` and `blocky` wherever their `*_subdomain` keys answer — withdraw them on hosts that must not serve them (see [Host-scoped Config](configuration/host-scoped-config.md)):

```toml
[hosts.agent-box]
headscale_subdomain = ""
blocky_subdomain = ""
```

## Step 3: Bootstrap, harden, infrastructure

!> Allow your custom `ssh_port` in the provider firewall **before** bootstrap.

```bash
auberge ansible bootstrap agent-box --ip 203.0.113.20
auberge ansible run --host agent-box --playbook ansible/playbooks/hardening.yml
auberge ansible run --host agent-box --playbook ansible/playbooks/infrastructure.yml
```

## Step 4: Enroll in the tailnet

With self-hosted Headscale and an [ACL policy](applications/networking/headscale.md), mint a pre-auth key carrying the host's trust tag and scope it to the host — the fleet-wide `tailscale_authkey` was consumed by your first host:

```bash
auberge headscale add-key -t tag:agent
```

```toml
[hosts.agent-box]
tailscale_authkey = "<generated key>"
```

Re-run the infrastructure playbook (or `-t tailscale`); the `tailscale` role authenticates against `tailscale_login_server` with the tagged key. The ACL policy, not the target host, decides what the node may reach.

## Step 5: Verify

```bash
ssh agent-box 'sudo ufw status && sudo fail2ban-client status sshd'
ssh agent-box 'tailscale status'
```

For a tag-confined node, prove the confinement from the node itself: connections to peers outside its allowed set must time out.
