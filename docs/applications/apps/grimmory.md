# Grimmory

Self-hosted multi-user digital library with OIDC auth, OPDS catalog, Kobo/KOReader sync, and BookDrop auto-import. Docs: [grimmory.org](https://grimmory.org)

- **URL**: `https://{subdomain}.{domain}`
- **Port**: 6060 (internal)
- **Data**: `/srv/books` (library, `grimmory_library_path`), `/srv/grimmory` (covers, thumbnails, BookDrop staging), `/srv/bookdrop` (auto-import drop)

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

Backed up by default (book library + app data + metadata database). See [Backup & Restore](backup-restore/overview.md).

Grimmory stores the library root as a row in its own database, not in a config file ansible writes, so the role's `grimmory_library_path` is a declaration nothing on the host enforces. The Backup Recipe's `attests:` query reads that row back on every `backup create` and fails when it names a path no declared path holds — see [ADR-0033](https://github.com/sripwoud/auberge/blob/master/meta/adr/0033-app-owned-paths-are-verified-not-trusted.md). Re-pointing the library in the UI therefore requires the same edit in `defaults/main.yml` and `grimmory.meta.yml`.
