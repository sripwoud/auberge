# Fleet GitHub Identity

The AI agent fleet ([ruche](https://github.com/sripwoud/auberge/issues/747)) commits and opens PRs as a **GitHub machine user**, never the personal account ([ADR-0054](https://github.com/sripwoud/auberge/blob/master/meta/adr/0054-agent-workloads-run-on-a-dedicated-disposable-host.md), [ADR-0056](https://github.com/sripwoud/auberge/blob/master/meta/adr/0056-github-machine-user-access-is-cli-provisioned.md)):

- **Blast-radius containment** — a credential leaked off a compromisable box acts only as the bot, never the owner, and touches no org membership.
- **Honest review boundary** — the bot cannot approve its own PRs as the owner.
- **Independent lifecycle** — the token expires and rotates on its own schedule; clean provenance on every commit.

GitHub ToS permits one machine account alongside a personal one.

## PR flow

| Repo class           | Mechanism                                                                                        | Provisioning                 |
| -------------------- | ------------------------------------------------------------------------------------------------ | ---------------------------- |
| Owned (`sripwoud/*`) | `push` collaborator → branches in origin, PR in-repo; `master` protected so the bot cannot merge | `auberge github invite`      |
| External             | bot forks → pushes to its fork → cross-repo PR                                                   | none (public forks are free) |

## Config keys

| Key                | Secret | Holds                                                                      |
| ------------------ | ------ | -------------------------------------------------------------------------- |
| `github_bot_login` | no     | the machine account handle                                                 |
| `github_bot_repos` | no     | space-separated owned allowlist, e.g. `sripwoud/auberge sripwoud/dotfiles` |
| `github_bot_token` | yes    | fine-grained PAT, e.g. `!pa show fleet/github-pat`                         |

## Provisioning

Account signup and token minting sit behind a login only a human holds — those steps are the checklist below. The two scriptable verbs are CLI commands reading the config above.

1. **Create the machine account** (browser, logged out of the owner): <https://github.com/signup>. Set `github_bot_login` to the handle; give it its own email.

2. **Set the allowlist** and invite the bot (as the owner — `invite` refuses to run when the active `gh` account is the bot):

   ```bash
   auberge config set github_bot_login <handle>
   auberge config set github_bot_repos "sripwoud/auberge sripwoud/dotfiles"
   auberge github invite
   ```

3. **Accept each invitation** as the bot: <https://github.com/notifications> (or the emailed link). `verify` flags any left pending.

4. **Mint a fine-grained PAT** as the bot: <https://github.com/settings/personal-access-tokens/new>. Resource owner: the bot. Repository access: **All repositories** (the real allowlist is enforced by the collaborator invites, not the token scope). Permissions: Contents **Read and write**, Pull requests **Read and write**, Metadata **Read-only**. Expiry: **90 days** — never no-expiration on a box assumed compromisable; note the rotation date.

5. **Store the token** and point config at it:

   ```bash
   pa add fleet/github-pat
   auberge config set github_bot_token "!pa show fleet/github-pat"
   ```

6. **Verify** — proves the token is the bot and reaches every allowlist repo (push confirmed via `.permissions.push`, since a public repo answers a bare read for any token):

   ```bash
   auberge github verify            # exit 0 = verified, 1 = a finding
   auberge github verify -o json    # { outcome, identity_ok, repos: [{repo, state}] }
   ```

7. ruche's meta role ([#743](https://github.com/sripwoud/auberge/issues/743)) resolves `github_bot_token` at deploy time and templates it onto the box (git author = bot).

## Rotation

Fine-grained tokens expire — 90 days is the ceiling. To rotate: regenerate on the [token page](https://github.com/settings/personal-access-tokens), `pa edit fleet/github-pat`, `auberge github verify`, then `auberge deploy ruche` to re-template.
