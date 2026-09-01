# ruche — agent host

You are running unattended on `ruche`, a disposable agent host. Nobody is watching the
session. Every permission is pre-approved except the rules below, so nothing stops you
that you do not stop yourself.

## The box

- Rebuilt from IaC. It holds no irreplaceable state: clones, worktrees and caches only.
- Isolated on the tailnet as `tag:agent`. The data hosts are unreachable from here by
  design — a failure to reach one is the policy working, not an outage to route around.
- Transcripts leave the box. Nothing on this host prevents you from printing a
  credential; treat every secret you can read as one you must not echo, quote or paste
  into a session.

## Denied, and why

`/etc/opencode/opencode.json` holds the deny list. These are the acts whose consequences
outlive the box, so a denial is a decision already taken, not an obstacle to solve.

| Denied                                            | Why                                              |
| ------------------------------------------------- | ------------------------------------------------ |
| `git push --force`, `-f`, `+ref:ref`              | rewrites history other people and CI depend on   |
| `sudo`, `doas`                                    | this host's config is not yours to edit          |
| root- and `$HOME`-anchored `rm -rf`, `mkfs`, `dd` | recoverable only by rebuilding the host          |
| reading `*.env` and `/etc/opencode/*`             | keeps credentials out of a transcript by default |

If a task genuinely needs one of these, stop and say so in the session. Do not reach for
an equivalent the rules do not name — a nested shell, a differently spelled flag, an
`opencode.json` of your own. Circumventing a denial is a worse outcome than the task
going unfinished, and it is the one thing here nobody can review after the fact.

## Working here

- Branch and open a PR. Never push to a default branch.
- Prefer relative paths inside your worktree; root-anchored deletes are denied.
- Leave the workspace clean: no stray branches, no uncommitted state you did not explain.
