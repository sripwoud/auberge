# ADR-0065: The agent's permission baseline is a guard rail, and the boundaries are elsewhere

## Status

Accepted, 2026-09-01. Decided in #741, under the ruche epic (#747). **Implements ADR-0054's** "permission config of `"*": "allow"` plus an explicit deny list" by deciding where that config lives and what may be claimed for it.

## Decision

**The `opencode` role deploys its permission baseline to `/etc/opencode/opencode.json` — OpenCode's managed config tier, root-owned, 0644 — and states the allow-all blanket per tool, never as a top-level `"*"`.** The OpenRouter credential reaches the runtime as a `{file:/etc/opencode/openrouter.key}` substitution against a 0640 root-owned sidecar, so the baseline itself carries no secret.

**The deny list is a guard rail against ordinary agent behaviour and accidental config, not a boundary against a deliberate attempt to lift it.** The boundaries that hold are elsewhere and are named below. This distinction is the substance of the ADR; getting it wrong is what the first draft of #741 did.

### Why the blanket is per tool

OpenCode does not evaluate `permission` as nested blocks. It flattens the whole object into one list of `{permission, pattern}` rules **in object key order** and decides with `findLast` — the last rule whose permission matches the tool and whose pattern matches the input. A top-level `"*": "allow"` is therefore not "the default for tools I did not mention"; it is a rule matching every tool and every input, and its position relative to the `bash` block decides whether the block means anything.

That position is not this repo's to control. Config sources merge as `{...lower, ...managed}`: a key the lower-precedence file also names keeps **that file's position** while taking the managed file's **value**, and every managed-only key is appended after it. So one benign project config is enough to sink the top-level rule below the entire deny list. Measured against the shipped 1.18.25 binary, with a project config of `{"permission":{"bash":{"harmless-noop":"deny"}}}`:

|        | rule              | index                             |
| ------ | ----------------- | --------------------------------- |
| before | `*` / `*` → allow | 0                                 |
| after  | `*` / `*` → allow | **17**, behind all sixteen denies |

At index 17 that rule is the last match for `sudo rm -rf /`, and every deny above it is inert. Stating the blanket inside `bash` and `read` instead removes the cross-tool rule entirely; what neither block names is allowed by OpenCode's own defaults, which are merged ahead of every config file and cannot be reordered by one. `tests/opencode_permission_baseline.rs` models the flattened `findLast` evaluation and fails the build if a cross-tool rule reappears.

### What is left, measured

With the cross-tool rule gone, a lower-precedence config that names a deny pattern **verbatim** hoists that one key above its block's catch-all and inverts it — for that pattern only. Measured: a project config of `{"permission":{"bash":{"sudo *":"allow"}}}` yields `sudo apt install x` → allow, while every other deny still holds. No formulation of the config avoids this, because the agent controls a config file's key order and the runtime decides by position.

So the honest reading of the deny list is: it stops the agent doing these things in the course of doing its job, and it survives any config that is not written to defeat it. It does not stop an agent that is trying.

### Where the boundaries actually are

| Harm                    | What holds it                                                                                                                                                                                            |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Reaching the data fleet | the tailnet ACL, `tag:agent` (ADR-0055) — enforced in the mesh, not on the box                                                                                                                           |
| Root on the box         | the admin user's sudoers entry requires a password the agent does not hold                                                                                                                               |
| Losing the box          | it is disposable by construction (ADR-0054); a rebuild is the recovery                                                                                                                                   |
| Rewriting `master`      | the repo's `main` ruleset — `non_fast_forward`, `deletion`, `required_linear_history`, PR-only with squash. Bypass is admin-only and the machine user is invited at `push` (#745), so it does not bypass |

Force-push is worth spelling out because it cannot be withheld where one would first look for it. GitHub has no "push but not force-push" grant: the `push` collaborator role and a `Contents: write` PAT both carry it, and withholding it would also stop the machine user rebasing its own branch, which is ordinary work. The decision is only expressible server-side, per branch — which is what `non_fast_forward` is. So the `git push --force` rail in the baseline is defence in depth over a boundary that already holds for the default branch, not the only thing standing there.

Two gaps remain, and neither is this role's to close. The ruleset targets `~DEFAULT_BRANCH` only, so force-pushing any _other_ branch — a colleague's, a release branch, the base of a stacked PR — is unprotected. And protection is per repository, while `github_bot_repos` is an allowlist: adding a repo to it grants push without asserting anything about that repo's rules. `auberge github verify` already proves push access and is the natural place to also prove the default branch refuses a rewrite.

Reading a secret is on neither list: `bash` is allow-all, so `cat /etc/opencode/openrouter.key` prints the key. The `read` denies keep `*.env` and the sidecar out of context **by default**, which is worth having because transcripts leave the box (ADR-0054) — but they are a default, not a control, and the AGENTS.md baseline says so to the agent in as many words.

## Why

ADR-0054 makes prompt injection the design assumption. Under that assumption, a claim that a config file constrains the agent is worse than no claim: it is the thing everything downstream gets sized against. The first draft of this ADR asserted the deny list "cannot be lifted", verified it by reading the _merged configuration_ back out of `opencode debug config`, and saw the value `deny` sitting where it was written. The merged value was correct and the verdict was `allow` — the reading never touched the evaluation. A review caught it; the measurements above are the repair.

The managed tier is still the right place to deploy, for the reason that survives: it is the only tier whose **values** outrank a project or user config, so the common case — a cloned repo shipping an `opencode.json` — cannot loosen a rule, and a rule this repo tightens actually reaches the Host. What changed is what is claimed for it.

## Trade-off

- **`sudo` is denied on a box built for unattended work.** An agent that genuinely needs a package has to stop and say so. Accepted, and cheap: the sudoers password requirement is the real control, so this rail costs little and removes the most common accidental path to editing `/etc/opencode`.
- **Root-anchored `rm -rf /*` is denied as a class**, which also stops a legitimate `rm -rf /home/agent/workspace/repo/node_modules`. Accepted: the denied call is recoverable by re-issuing it relative to the worktree, and the pattern that would allow it is the pattern that allows `rm -rf /etc`.
- **`external_directory` is set to `allow`.** Its default is `ask`, and nobody is present to answer; a worktree-per-agent layout crosses directory boundaries constantly. This widens what the agent may touch on a box where that is already assumed.
- **No checksum on the download.** Upstream publishes `sha512` only for the desktop artifacts; the CLI tarball has none and its filename is version-agnostic, so integrity rests on HTTPS to the release URL. A pinned literal hash was rejected: Renovate bumps the version in the Playbook Meta and would leave the hash stale, turning every bump into a failed deploy.

## Considered alternatives

- **`~/.config/opencode/opencode.json` (the global tier).** The location the upstream docs lead with. Rejected: the agent owns its home, so the baseline would not survive even a careless session, and a rule tightened in this repo could be overridden by a cloned repo's own config.
- **Drop the `bash` catch-all too**, leaving only denies, so a hoisted key cannot be re-allowed by a catch-all beneath it. Rejected on measurement: a two-key crafted config (`{"sudo *": …, "*": "allow"}`) defeats that shape as well, so it buys one key of difficulty and loses the `"*": "allow"` #741 asks for and the legibility that comes with it.
- **`opencode --auto` instead of a config baseline.** ADR-0054 rejected the blanket flag already, and the reason is visible here: `--auto` offers nowhere to put the denies, so they would need a config file regardless.
- **The key inline in the config.** One fewer file. Rejected: it makes the baseline a secret, so it can no longer be read, diffed or grepped without care. The `{file:…}` seam is upstream's documented answer and is verified working in the managed tier.
- **The key as an `/etc/environment` variable**, which is what the `claude_code` role does. Rejected: world-readable, and ADR-0054 already records that role's `/etc/environment` writes as a trap not to reuse.

## Consequences

**Positive:**

- The one defect that would silently void the whole baseline — a cross-tool catch-all — is now impossible to reintroduce without failing the build, and the fence explains why in the failure message.
- The fence decides verdicts by simulating `findLast` over the flattened rule list, so it catches a reordering that leaves every rule present and every rule inert. Nine mutations were checked; each is red.
- The baseline is one non-secret file an operator can `cat` on the box and compare against the repo.
- #744 extends the runtime through the `opencode_plugins` default, so adding `@zilliz/memsearch-opencode` is a list entry and never an edit to the permission model beside it.

**Negative:**

- The deny list is defeatable by a crafted config, and this ADR is the only place that says so. Anything built on top of it that needs a real boundary has to reach for the table above.
- Force-pushing a _non-default_ branch has no boundary, and no repo in `github_bot_repos` is checked for having any rules at all. The baseline's rail is the only thing there.
- A deny rule can only be relaxed by a deploy, so a genuinely blocked task waits for one.
- The merge-order and `findLast` behaviours are upstream's implementation, observed here rather than contracted. A version bump could change either silently; nothing in CI would notice, because the fence reasons about the repo's own file.

## References

- ADR-0054 — the agent Host, and the runtime decision this one implements.
- ADR-0055 — the tailnet ACL: the boundary that does hold.
- ADR-0045 — required keys declared in Playbook Meta; `opencode_openrouter_api_key` is declared there and is never a repo literal.
- ADR-0027 — the install regime. The role reads its installed version from `opencode --version`, so a deleted binary reads as nothing installed.
- Issue #741, epic #747. Issue #745 for the machine user, invited at `push`; the repo's `main` ruleset is what makes that safe on the default branch.
