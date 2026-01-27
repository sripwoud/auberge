# auberge select playbook

Interactively select an Ansible playbook

## Synopsis

```bash
auberge select playbook
```

## Alias

`auberge se p`

## Description

Presents an interactive list of Ansible playbooks for selection. Outputs the selected playbook path to stdout.

Discovers playbooks from ansible/playbooks/ directory.

## Examples

```bash
# Select playbook
auberge select playbook

# Use in command substitution
PLAYBOOK=$(auberge select playbook)
echo "Selected: $PLAYBOOK"

# Use with other commands
auberge ansible run --host myserver --playbook $(auberge select playbook)

# In shell scripts
#!/bin/bash
PLAYBOOK=$(auberge select playbook)
for host in $(auberge host list -o json | jq -r '.[].name'); do
  auberge ansible run --host "$host" --playbook "$PLAYBOOK"
done
```

## Interactive Selection

```
? Select playbook
  Bootstrap (bootstrap.yml)
  Applications (apps.yml)
> Full Stack (auberge.yml)
  Freshrss Only (freshrss.yml)

[↑↓ to move, Enter to select, Esc to cancel]
```

## Output

Outputs the full playbook path:

```
/home/user/code/auberge/ansible/playbooks/auberge.yml
```

Exit codes:

- 0: Playbook selected
- 1: No playbook selected (Esc pressed) or error

## Playbook Discovery

Searches for playbooks in:

- ansible/playbooks/*.yml
- Relative to project root

Recognized playbooks:

- bootstrap.yml
- apps.yml
- auberge.yml
- Individual app playbooks (freshrss.yml, radicale.yml, etc.)

## Use Cases

**Interactive deployment**:

```bash
HOST=$(auberge select host)
PLAYBOOK=$(auberge select playbook)
auberge ansible run --host "$HOST" --playbook "$PLAYBOOK"
```

**Shell script menu**:

```bash
#!/bin/bash
echo "Select playbook to run:"
PLAYBOOK=$(auberge select playbook)

echo "Select target host:"
HOST=$(auberge select host)

echo "Running $PLAYBOOK on $HOST"
auberge ansible run --host "$HOST" --playbook "$PLAYBOOK"
```

**Quick playbook runner**:

```bash
# In shell alias or function
function aplay() {
  auberge ansible run \
    --host "$(auberge select host)" \
    --playbook "$(auberge select playbook)"
}
```

## Playbook Metadata

Playbooks show descriptive names in the selector:

- File name shown in parentheses
- Display name derived from playbook metadata or filename

## Related Commands

- [auberge select host](host.md) - Select host interactively
- [auberge ansible run](../ansible/run.md) - Run playbook

## See Also

- [Ansible Playbooks](../../core-concepts/ansible.md)
- [Deployment](../../deployment/README.md)
