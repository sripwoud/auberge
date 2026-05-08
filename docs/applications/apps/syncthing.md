# Syncthing

Continuous file synchronization between devices. Docs: [syncthing.net](https://syncthing.net)

- **URL**: web UI via SSH tunnel only (not publicly exposed)
- **Port**: 8384 (local tunnel target)
- **Data**: configured sync folders on VPS

## Deploy

```bash
auberge deploy syncthing
```

## Notes

Access the web UI via SSH tunnel:

```bash
ssh -L 8384:localhost:8384 user@vps
```

Then open http://localhost:8384.

Optional workspace sync (per-host config):

```yaml
syncthing_configure_workspace: true
syncthing_device_id: "XXXXXXX-XXXXXXX-..."
```
