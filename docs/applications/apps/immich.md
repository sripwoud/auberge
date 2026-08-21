# Immich

Self-hosted photo and video management. The repo's one containerized App: upstream supports only docker compose, so the official compose stack (server, machine-learning, valkey, postgres) runs wrapped in a systemd unit. Docs: [immich.app](https://immich.app)

- **URL**: `https://{subdomain}.{domain}`
- **Data**: originals under `/var/lib/immich/upload`, postgres cluster under `/var/lib/immich/postgres`

## Deploy

```bash
auberge deploy immich
```

Requires `immich_db_password` (A-Za-z0-9 only — upstream constraint) and `immich_subdomain` in `config.toml`.

## Notes

`systemctl stop immich` runs `docker compose down`, so one unit stops the whole stack. Immich deliberately declares no Backup Recipe: the photo library is orders of magnitude larger than any other App's state, and the pull-based `auberge backup` flow would re-transfer all of it through the operator's machine on every run with the stack stopped. Its offsite backup instead runs on the Host itself — a nightly timer dumping the database live and pushing it with the upload directory straight to a dedicated B2 bucket via restic ([#558](https://github.com/sripwoud/auberge/issues/558)). All container ports bind to `127.0.0.1`; Caddy proxies the public subdomain.
