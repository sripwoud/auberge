# ADR-0056: The GitHub machine user's access is provisioned by the CLI, from declared config

## Status

Accepted, 2026-08-31. Decided in #745, under the ruche epic (#747, ADR-0054).

## Decision

**The fleet's GitHub machine user (ADR-0054) is provisioned by `auberge github invite|verify`, two CLI verbs reading declared config — not a reference script.** The shape:

- **Owned-only declared allowlist.** `github_bot_repos` (a space-separated key, the `fail2ban_ignore` shape) lists just the `sripwoud/*` repos the bot gets a `push` invite on; `github_bot_login` names the account; `github_bot_token` holds a fine-grained PAT via the config `!`-resolver (`!pa show fleet/github-pat`). External repos are not declared.
- **Hybrid PR flow, keyed on ownership.** Owned repos: the bot is a `push` collaborator, pushes branches to origin, opens PRs in-repo; `master` protection plus the no-self-approve boundary keep it from merging. External repos: the bot forks and opens cross-repo PRs — no owner-side provisioning, because a public fork needs none.
- **`invite`** runs as the owner (ambient `gh` auth) and refuses when the active account is the bot; `gh api PUT …/collaborators/…` per repo, re-run when a repo is added.
- **`verify`** runs as the bot (its token through `GH_TOKEN`), proves the token authenticates as `github_bot_login`, and classifies each allowlist repo `reachable` / `pending` / `unreachable`. Push is read off `GET /repos/{repo}`'s `.permissions.push`, not mere reachability: a public owned repo answers a bare read for any token, so only the authenticated push permission proves the invitation was accepted. `--output {human,json}` per ADR-0004 (`outcome` discriminator per ADR-0044); exit 0 verified, 1 a finding.
- **Single all-repositories PAT.** Resource owner the bot, Contents + Pull-requests RW, Metadata RO, 90-day expiry — never no-expiration on a box assumed compromisable. The per-repo allowlist is enforced at the _collaboration_ layer (who the bot is invited to), not the token scope: the token only ever exercises access the bot already has — invited owned repos plus its own forks.

Account signup, token minting, and invitation acceptance are irreducibly manual and stay a docs checklist (`docs/configuration/fleet-github-identity.md`). The token reaches ruche through the config `!`-ref and that role's templating (#743), not through this command.

## Why this is non-obvious

The reference-script form was built first and rejected under review. The repo's own boundary put the immich-B2/bichon laptop scripts in `examples/` because they compose building blocks with the operator's own tooling and auberge "knows nothing about any secret store or config tool." That argument flips once the allowlist is _declared config_: `verify` becomes a reconcile check in the family of `dns status` / `backup verify`, `invite` reads the same declared set, and both reuse machinery the binary already owns — `resolve_value` (so nothing hardcodes `pa`, unlike the script), `validate_required`, ADR-0004 output. A script would now be the inconsistent choice. `invite` is not purely one-shot either — it re-runs whenever an owned repo is added — so it clears ADR-0002's deletion test.

## Alternatives considered

- **A reference script in `examples/`** (the immich-B2 precedent, this decision's first form). Rejected: it hardcodes `pa show` — the only place in the tree naming a secret tool — and cannot reach `resolve_value`, `validate_required`, or the `--output` surface; with the allowlist declared, it duplicates state the CLI already reads.
- **Only `verify` in the CLI, `invite` a script.** Rejected: splits one procedure across two homes for no gain once both verbs read the same declared allowlist.
- **`--output` on `invite`.** Rejected by ADR-0004's load-bearing-field rule: the invite outcome is success/failure carried by the exit code, and the run bails on the first failure.
- **A `keys.yml` allowlist enumerating external fork targets, or a `github_owner` classification key.** Rejected: external repos churn and public forks need no provisioning, so the config stays the stable owned push-set and classification is implicit.
- **"Only select repositories" PAT scope.** Rejected: it cannot reach the bot's future forks, breaking the external fork-flow; the collaborator invite is the real allowlist, so the wider token buys simplicity at no extra sensitive reach.
