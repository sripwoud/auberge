# Grimmory

Self-hosted multi-user digital library with OIDC auth, OPDS catalog, Kobo/KOReader sync, and BookDrop auto-import. Docs: [grimmory.org](https://grimmory.org)

- **URL**: `https://{subdomain}.{domain}`
- **Port**: 6060 (internal)
- **Data**: `/srv/grimmory` (library), `/srv/bookdrop` (auto-import drop)

## Deploy

```bash
auberge deploy grimmory
```

?> Run the `build-grimmory` GitHub Action to build the JAR artifact before the first deploy.

## Required config

| Key                    | Purpose          |
| ---------------------- | ---------------- |
| `grimmory_db_password` | MariaDB password |

## Notes

Backed up by default (book library + metadata database). See [Backup & Restore](backup-restore/overview.md).
