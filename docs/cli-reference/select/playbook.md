# auberge select playbook

Interactively select an Ansible playbook and print its full path to stdout. Alias: `auberge se p`.

```bash
auberge select playbook
```

Discovers playbooks from `ansible/playbooks/*.yml` relative to the project root. Most commands invoke this automatically when `--playbook` is omitted. Use directly for shell scripting.

## Examples

```bash
PLAYBOOK=$(auberge select playbook)
auberge ansible run --host myserver --playbook "$(auberge select playbook)"
```

Exit 0 on selection, 1 on cancel (Esc) or error.
