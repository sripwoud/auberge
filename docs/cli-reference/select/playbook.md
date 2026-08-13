# auberge select playbook

Interactively select an Ansible playbook and print its full path to stdout. Alias: `auberge se p`.

```bash
auberge select playbook
```

Discovers `*.yml` in the extracted asset tree at `$XDG_DATA_HOME/auberge/ansible/playbooks/` — or `./ansible/playbooks/` when `AUBERGE_DEV=1`. `*.meta.yml` sidecars are excluded.

`ansible run` opens this same picker when `--playbook` is omitted, so wrapping a single call in `$(...)` buys nothing. Reach for it to pick **once** and reuse the path — across several commands, or in a tool outside auberge.

The picker draws on stderr, so it stays visible when stdout is captured or piped.

## Examples

Pick once, run it against several hosts:

```bash
PLAYBOOK=$(auberge select playbook)
auberge ansible run -H auberge -p "$PLAYBOOK"
auberge ansible run -H openclaw -p "$PLAYBOOK"
```

Inspect the resolved file:

```bash
ansible-lint "$(auberge select playbook)"
yq '.[0].roles' "$(auberge select playbook)"
```

!> The printed path points at the extracted copy, not your checkout. Edits there are overwritten on the next version bump — change playbooks in the repo.

Exit 0 on selection, 1 on cancel (Esc) or error.
