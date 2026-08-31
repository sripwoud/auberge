# Fleet GitHub Identity

The AI agent fleet ([ruche](https://github.com/sripwoud/auberge/issues/747)) commits and opens PRs as a **GitHub machine user**, never the personal account ([ADR-0054](https://github.com/sripwoud/auberge/blob/master/meta/adr/0054-agent-workloads-run-on-a-dedicated-disposable-host.md)):

- **Blast-radius containment** — a credential leaked off a compromisable box acts only as the bot, never as the owner, and touches no org membership.
- **Honest review boundary** — the bot cannot approve its own PRs as the owner.
- **Independent lifecycle** — the token expires and rotates on its own schedule; clean provenance on every commit.

GitHub ToS permits one machine account alongside a personal one.

## Provisioning

Account creation and fine-grained-token minting sit behind a login only a human holds, so those stay a checklist; inviting the allowlist repos and verifying the stored token are scripted. Both live in [`examples/github-machine-user.sh`](https://github.com/sripwoud/auberge/blob/master/examples/github-machine-user.sh):

```bash
examples/github-machine-user.sh            # print the ordered checklist
examples/github-machine-user.sh invite     # invite the bot to every allowlist repo
examples/github-machine-user.sh verify     # prove the stored token is the bot
```

`invite` runs as the **owner** and refuses when the active `gh` account is the bot — the machine user never provisions itself.

## Least privilege

| Axis                     | Value                            | Why                                                                         |
| ------------------------ | -------------------------------- | --------------------------------------------------------------------------- |
| Repo allowlist           | `FLEET_REPO_ALLOWLIST`, per-repo | The token reaches only enrolled repos, nothing else                         |
| Collaborator permission  | `push` (default)                 | The least that pushes a branch and opens a PR — `pull`/`triage` cannot push |
| Token: Contents          | Read and write                   | Clone, push branches                                                        |
| Token: Pull requests     | Read and write                   | Open and update PRs                                                         |
| Token: Metadata          | Read-only                        | Mandatory, added automatically                                              |
| Token: Repository access | Only select repositories         | Scoped to the allowlist                                                     |

## Storage and rotation

The token is stored in [`pa`](secrets.md#password-commands) — it never passes through the script; the human runs `pa add`, and `verify` reads it back through `pa show`:

```bash
pa add fleet/github-pat
```

`config.toml` references it, and ruche's meta role ([#743](https://github.com/sripwoud/auberge/issues/743)) templates it onto the box at deploy time:

```toml
ruche_github_token = "!pa show fleet/github-pat"
```

Fine-grained tokens expire — 90 days is the sane ceiling. To rotate: regenerate on the [token page](https://github.com/settings/personal-access-tokens), `pa edit fleet/github-pat`, rerun `verify`, then `auberge deploy ruche` to re-template.
