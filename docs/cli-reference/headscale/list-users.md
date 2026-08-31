# auberge headscale list-users

List registered Headscale users

## Synopsis

```bash
auberge headscale list-users [OPTIONS]
```

## Alias

`auberge hs lu`

## Description

Lists all users registered on the target Headscale instance.

## Options

| Option                | Description                     | Default                   |
| --------------------- | ------------------------------- | ------------------------- |
| `--host HOST`         | Target host running headscale   | Serving host, else prompt |
| `-o, --output FORMAT` | Output format (`human`, `json`) | `human`                   |

## Examples

```bash
# List users on a host
auberge headscale list-users --host myserver

# Output as JSON
auberge headscale list-users --host myserver --output json
```

## Output Example

```
ID  NAME     CREATED
1   default  2024-01-15 10:00:00 UTC
2   mobile   2024-01-20 14:30:00 UTC
```

## JSON Output

```bash
auberge headscale list-users --host myserver --output json
```

```json
[
  {
    "id": 1,
    "name": "default",
    "created_at": { "seconds": 1705312800, "nanos": 0 }
  },
  {
    "id": 2,
    "name": "mobile",
    "created_at": { "seconds": 1705760400, "nanos": 0 }
  }
]
```

A subset of headscale's own user record, re-emitted under its field names — `created_at` stays the `google.protobuf.Timestamp` headscale sends rather than an ISO string. The human table renders it as UTC.

JSON goes to stdout; human-format chrome (banners, info messages) goes to stderr.

**Schema**

| Field        | Type   | Description                            |
| ------------ | ------ | -------------------------------------- |
| `id`         | number | Headscale user ID                      |
| `name`       | string | User name                              |
| `created_at` | object | `google.protobuf.Timestamp`, or `null` |
