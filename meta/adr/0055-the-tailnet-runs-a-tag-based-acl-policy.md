# ADR-0055: The tailnet runs a tag-based ACL policy

## Status

Accepted, 2026-08-31. Decided in #747; implemented by #738.

## Decision

**headscale gets a policy file, config-owned per the ADR-0051 pattern, with tag-based groups as the unit of trust:**

| Tag           | Nodes             | May initiate                             |
| ------------- | ----------------- | ---------------------------------------- |
| `tag:trusted` | lechuck, pixel-9a | everything                               |
| `tag:data`    | auberge           | what it serves; never toward `tag:agent` |
| `tag:agent`   | ruche             | internet only — nothing tailnet-side     |
| `tag:standby` | vieille-auberge   | as today (rollback surface)              |

All tags reach the Blocky global resolver on 53 (ADR-0052). The ACL tags `auberge headscale add-key -t` already stamps on pre-auth keys become consumed rather than decorative.

## Why

The tailnet has run headscale's default allow-all since the #712 flag day — no `policy:` block exists anywhere in the role, its templates, or config. That was acceptable while every node was a trusted device. ADR-0054 enrolls a node _designed on the assumption of compromise_: an unconfined `ruche` is a pivot from a YOLO agent box to photos, documents, and mail. Confinement must live in the mesh, not in the target hosts — ufw on auberge cannot distinguish one tailnet peer from another.

This is its own ADR, not a rider on ADR-0054, because the first policy is fleet-wide by nature: headscale flips from default-allow to default-deny the moment a policy file exists, so the decision constrains every current and future node, and it outlives any one box.

## Trade-off

The default-deny flip risks breaking flows that were never inventoried — syncthing lechuck↔auberge, the laptop's backup pulls, tailnet-only app vhosts, SSH. Accepted with a rollout gate in #738: enumerate and verify the flows before and after, and enroll `ruche` only once the policy is live, so no flat-tailnet window ever exists.

## Alternatives considered

- **Keep allow-all, rely on host firewalls** — rejected: per-peer distinction is exactly what ufw on the target lacks; one misconfigured box is an open lateral move.
- **Keep `ruche` off the tailnet** (public HTTPS or tunnel for the dashboard) — rejected: trades a mesh under our control for public exposure, and loses lechuck-initiated syncthing for the transcript sync.
- **Per-node headscale users/namespaces as the isolation unit** — rejected: coarser than tags, fights the MagicDNS/split-DNS layout, and the CLI already stamps tags that map 1:1 to the trust tiers.
