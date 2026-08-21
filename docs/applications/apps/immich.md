# Immich

Self-hosted photo and video management. The repo's one containerized App: upstream supports only docker compose, so the official compose stack (server, machine-learning, valkey, postgres) runs wrapped in a systemd unit. Docs: [immich.app](https://immich.app)

- **URL**: `https://{subdomain}.{domain}`
- **Data**: originals under `/var/lib/immich/upload`, postgres cluster under `/var/lib/immich/postgres`

## Deploy

```bash
auberge deploy immich
```

Requires `immich_db_password` (A-Za-z0-9 only — upstream constraint), `immich_subdomain`, and the four backup keys — `immich_restic_repository`, `immich_restic_password`, `immich_b2_key_id`, `immich_b2_application_key` — in `config.toml`.

## Backup

Nightly on-host offsite backup to a dedicated B2 bucket ([#558](https://github.com/sripwoud/auberge/issues/558)): `immich-backup.timer` (04:15, randomized ≤30min) dumps the containerized database live (`pg_dumpall` is MVCC-consistent, zero downtime) and pushes the dump plus `/var/lib/immich/upload` in one restic snapshot. A failed run is a failed unit — the Cockpit surface.

The box key is a crippled B2 application key created without `deleteFiles`, so a compromised Host can add but never destroy:

```bash
b2 key create --bucket <bucket> immich-box listBuckets,listFiles,readFiles,writeFiles
```

Retention and the staleness watchdog run weekly from the laptop with the full key: [`examples/immich-b2-prune.sh`](https://github.com/sripwoud/auberge/blob/master/examples/immich-b2-prune.sh) (install instructions in its header).

### First seed

Before the timer's first tick — the initial upload is ~21 GiB and worth supervising:

```bash
ssh <host>
sudo su -
set -a; source /opt/immich/immich-backup.env; set +a
restic init                          # once, creates the repository
systemctl start immich-backup.service
journalctl -fu immich-backup.service
restic check
```

### Restore

The snapshot holds the database dump and the photo library; postgres rebuilds its cluster from the dump, adapted from [upstream's procedure](https://immich.app/docs/administration/backup-and-restore):

```bash
auberge deploy immich                # stack up, empty database
set -a; source /opt/immich/immich-backup.env; set +a
restic restore latest --target /
systemctl stop immich
rm -rf /var/lib/immich/postgres/*    # dump restore needs a fresh cluster
cd /opt/immich && docker compose up -d database
gunzip -c /var/lib/immich/db-dump/immich-db.sql.gz \
  | sed "s/SELECT pg_catalog.set_config('search_path', '', false);/SELECT pg_catalog.set_config('search_path', 'public, pg_catalog', true);/g" \
  | docker exec -i immich_postgres psql --username=postgres --dbname=postgres
docker compose down
systemctl start immich
```

`restic restore --target /` places both directories at their original absolute paths; restic runs as root, so ownership survives.

## Notes

`systemctl stop immich` runs `docker compose down`, so one unit stops the whole stack. Immich deliberately declares no Backup Recipe: the photo library is orders of magnitude larger than any other App's state, and the pull-based `auberge backup` flow would re-transfer all of it through the operator's machine on every run with the stack stopped — offsite backup runs from the store of record instead (see [Backup](#backup)). All container ports bind to `127.0.0.1`; Caddy proxies the public subdomain.
