# Syncthing

Continuous file synchronization between devices.

Official Documentation: [https://syncthing.net](https://syncthing.net)

## Deployment

```bash
auberge ansible run --tags syncthing
```

## Configuration

Optional workspace sync can be enabled per-host:

```yaml
syncthing_configure_workspace: true
syncthing_device_id: "XXXXXXX-XXXXXXX-XXXXXXX-XXXXXXX-XXXXXXX-XXXXXXX-XXXXXXX-XXXXXXX"
```

## Web UI Access

Access via SSH tunnel (recommended):

```bash
ssh -L 8384:localhost:8384 user@vps
```

Then open http://localhost:8384 locally.

## Related

- [Applications Overview](../overview.md)
