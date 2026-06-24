# auberge host show

Show detailed host information

## Synopsis

```bash
auberge host show [OPTIONS] [NAME]
# Alias: auberge h s
```

## Description

Displays detailed configuration for a specific host, including all metadata and connection details.

If `NAME` is omitted, you'll be prompted to select a host.

## Arguments

| Argument | Description                     |
| -------- | ------------------------------- |
| NAME     | Host name (omit to be prompted) |

This command has no `--output` flag. Output is always YAML. To get machine-readable JSON for a specific host, use `auberge host list --output json | jq '.[] | select(.name=="myserver")'`. See [ADR-0004](https://github.com/sripwoud/auberge/blob/master/meta/adr/0004-cli-structured-output.md) for the reasoning.

## Examples

```bash
# Show host details
auberge host show myserver

# Interactive (prompts for host)
auberge host show
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
