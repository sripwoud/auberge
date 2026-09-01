# OpenCode

The agent runtime on [ruche](https://github.com/sripwoud/auberge/issues/747), the disposable agent Host. Model-agnostic, driven through OpenRouter. Docs: [opencode.ai/docs](https://opencode.ai/docs/)

- **URL**: no public URL — a CLI, started per session
- **Data**: nothing durable. Config in `/etc/opencode/`, workspace in `~/workspace/`

## Deploy

```bash
auberge ansible run opencode -H ruche
```

## Required config

| Key                           | Purpose                                                            |
| ----------------------------- | ------------------------------------------------------------------ |
| `opencode_openrouter_api_key` | OpenRouter API key — the only model credential on the Host         |
| `admin_user_name`             | The user agents run as; the workspace root resolves under its home |

Mint the key at [openrouter.ai/keys](https://openrouter.ai/keys) and store it the way every other secret is stored — a `!` command reference in `config.toml`, never a literal.

## Permission baseline

The role deploys `/etc/opencode/opencode.json`: allow-all per tool plus an explicit deny list, per [ADR-0065](https://github.com/sripwoud/auberge/blob/master/meta/adr/0065-the-agent-permission-baseline-is-a-guard-rail-not-a-boundary.md).

| Denied                                            | Why                                              |
| ------------------------------------------------- | ------------------------------------------------ |
| `sudo`, `doas`                                    | keeps the box's own config out of ordinary reach |
| `git push --force` / `-f` / `+ref:ref`            | rewrites history other people and CI depend on   |
| root- and `$HOME`-anchored `rm -rf`, `mkfs`, `dd` | recoverable only by rebuilding the Host          |
| reading `*.env` and `/etc/opencode/*`             | keeps credentials out of transcripts by default  |

!> **This is a guard rail, not a boundary.** It stops an agent doing these things in the course of its work; it does not stop one that is trying. A config source the agent can write which names a deny pattern verbatim hoists that key above its block's catch-all and inverts it — measured against 1.18.25 and recorded in ADR-0065. The boundaries that hold are the tailnet ACL (`tag:agent`), the sudoers password requirement, the box being disposable, and — for `master` — this repo's `main` ruleset (`non_fast_forward`, admin-only bypass, machine user invited at `push`). Force-pushing a _non-default_ branch, or any branch in a repo with no rules of its own, is covered by nothing but the rail above.

`bash` is allow-all, so the `read` denies do **not** stop `cat` reaching `.env` or the key file. They keep those out of context by default, which matters because transcripts leave the box.

### Rule order is load-bearing

OpenCode flattens the whole `permission` object into one list in key order and takes the **last** match. Two consequences the file's shape hides:

- The catch-all `"*": "allow"` must be **first** in each block.
- There must be **no top-level `"*"`**. It matches every tool, and merging with any lower-precedence config that mentions the same tool sinks it below the whole deny list — measured moving from index 0 to index 17, behind all sixteen denies, on a project config that only set `{"bash":{"harmless-noop":"deny"}}`.

`tests/opencode_permission_baseline.rs` simulates that evaluation and fails the build on either.

## Notes

The OpenRouter key never appears in the config. It lands in `/etc/opencode/openrouter.key` (mode `0640`, root-owned, agent-readable) and the config references it as `{file:/etc/opencode/openrouter.key}`.

`AGENTS.md` is deployed root-owned to the workspace root and named in the config's `instructions`, so it loads in every session regardless of where one starts.

Model routing defaults to `openrouter/anthropic/claude-sonnet-5`, with `openrouter/anthropic/claude-haiku-4.5` for lightweight calls. Override with `opencode_model` / `opencode_small_model`.

```bash
opencode debug config    # the resolved configuration, managed tier included
opencode --version       # what the install guard reads back
```
