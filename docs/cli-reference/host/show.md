# auberge host show

Show detailed host information

## Synopsis

```bash
auberge host show [OPTIONS] <NAME>
```

## Description

Displays detailed configuration for a specific host, including all metadata and connection details.

## Arguments

| Argument | Description          |
| -------- | -------------------- |
| NAME     | Host name (required) |

## Options

| Option              | Description               | Default |
| ------------------- | ------------------------- | ------- |
| -o, --output FORMAT | Output format: yaml, json | yaml    |

## Examples

```bash
# Show host details (YAML)
auberge host show myserver

# JSON output
auberge host show myserver --output json
```

## Output Example

```yaml
name: myserver
address: 192.168.1.10
user: ansible
port: 2222
ssh_key: ~/.ssh/identities/ansible_myserver
tags:
  - production
  - web
description: Production web server
python_interpreter: null
become_method: sudo
```

## Related Commands

- [auberge host list](list.md) - List all hosts
- [auberge host add](add.md) - Add a new host
- [auberge host edit](edit.md) - Edit host configuration
- [auberge host remove](remove.md) - Remove a host

## See Also

- [Host Configuration](../../configuration/hosts.md)
