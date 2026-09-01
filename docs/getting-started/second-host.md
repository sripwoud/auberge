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

Nothing to do by hand. The infrastructure run above already enrolled the host: it saw the target missing from Headscale's node listing, minted a 10-minute pre-auth key stamped with the host's `tailnet_tag`, and handed it to the `tailscale` role ([ADR-0063](https://github.com/sripwoud/auberge/blob/master/meta/adr/0063-a-pre-auth-key-is-minted-per-run-not-stored.md)). The ACL policy, not the target host, decides what the node may reach.

That works because the host declared its trust tier when you added it:

```bash
auberge host add agent-box --ip 203.0.113.20 --tailnet-tag agent
```

?> A host with no `tailnet_tag` still enrolls, but its key carries no tag — under the default-deny [ACL policy](applications/networking/headscale.md) it reaches nothing. The run warns and names `auberge host edit`.

## Step 5: Verify

```bash
ssh agent-box 'sudo ufw status && sudo fail2ban-client status sshd'
ssh agent-box 'tailscale status'
```

For a tag-confined node, prove the confinement from the node itself: connections to peers outside its allowed set must time out.
