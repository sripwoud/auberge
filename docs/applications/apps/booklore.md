# Booklore

Self-hosted multi-user digital library with smart shelves, auto metadata, and built-in reader for EPUB, PDF, and comics.

Official Documentation: [https://booklore.org](https://booklore.org)

## Prerequisites

1. Run the `build-booklore` GitHub Action to build the JAR artifact before deployment.
2. Set the database password:

```bash
mise set --age-encrypt --prompt BOOKLORE_DB_PASSWORD
```

## Deployment

```bash
auberge ansible run --tags booklore
```

## Configuration

| Setting  | Default       |
| -------- | ------------- |
| Port     | 6060          |
| Data     | /srv/booklore |
| BookDrop | /srv/bookdrop |
| JVM heap | 768 MB max    |

## Features

- Multi-user with OIDC authentication
- EPUB, PDF, CBZ, CBR, MOBI, and AZW3 support
- Kobo and KOReader sync
- OPDS catalog
- BookDrop auto-import
- Auto metadata from Google Books, Open Library, and Amazon

## Backup

Backed up by default. Includes:

- Book library
- Metadata database

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
