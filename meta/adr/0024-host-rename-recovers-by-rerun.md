# ADR-0024: Host rename recovers by rerun, never by rollback or history rewrite

## Status

Accepted, 2026-08-20.

## Context

`auberge host rename <old> <new>` (#520) changes a host's identity across three surfaces: the remote machine (`hostnamectl`, `/etc/hosts`), the local key directory (`~/.ssh/identities/<old>` → `<new>`, the #517 layout), and the `hosts.toml` entry with its `ssh_key` path. Any of the steps can fail independently, so the command needs a failure model. Two were considered:

1. **Transactional rollback** — on failure, undo the steps already done (rename the hostname back, move the key directory back).
2. **Rerun as recovery** — order the steps so every partial state re-passes preflight, make each step a no-op when already done, and tell the operator to rerun.

Rollback compounds the problem: the undo path runs exactly when something is already wrong (SSH flaking, disk full), doubles the states to reason about, and can itself fail halfway. A state file or journal to track progress adds a second source of truth about rename progress that can drift from the world it describes.

The same question reappears in backup history: restic snapshots group by host name, so after a rename the `<old>` lineage stops growing. Rewriting old snapshot tags (`restic tag --set`) would simulate continuity — a rollback of history.

## Decision

- **Remote first.** The remote steps run before any local mutation, so a remote failure aborts with zero local change and the rerun is a clean retry.
- **The hosts.toml write is the commit point.** Locally the key-directory move happens first and the hosts.toml write last. Every state before the write still names `<old>` in hosts.toml and re-passes preflight; every completed step no-ops on rerun (`hostnamectl` and the `/etc/hosts` sed are idempotent, the `mv` is skipped when `identities/<old>` is gone).
- **The `identities/<new>` collision check is conditional.** It bails only while `identities/<old>` also exists (a real clobber); "`<new>` present, `<old>` gone" is the already-moved state a rerun recovers through. The SSH preflight likewise probes the key at both the old and new locations.
- **Restic lineage freezes at rename.** The `<old>` snapshot group stays as-is and `<new>` starts a new one. Snapshot tags are never rewritten — the command prints this as a warning instead of "fixing" it.

## Consequences

**Positive:**

- One code path for first run and recovery; the fix for any partial failure is the command itself.
- No undo code, no state file, no way for a failed rollback to leave a third state.
- Backup history stays honest: a snapshot's host tag records the name the machine had when it was taken, which is what a restore ceremony (#513) keys on.

**Negative / accepted:**

- Between the remote step and the hosts.toml write, the remote hostname and the local config disagree. The window is seconds long and closed by rerunning.
- The frozen `<old>` lineage ages out via retention rather than being pruned as one group with `<new>`.
- Deliberately out of scope, printed as follow-ups: `~/.ssh/config` (the CLI treats it as read-only) and the tailnet name (tailscale re-advertises the new hostname on its own).
