# Grimmory

Self-hosted multi-user digital library with smart shelves, auto metadata, and built-in reader for EPUB, PDF, and comics.

Official Documentation: [https://grimmory.org](https://grimmory.org)

## Prerequisites

1. Run the `build-grimmory` GitHub Action to build the JAR artifact before deployment.
2. Set the database password:

```bash
auberge config set grimmory_db_password your-password
```

## Deployment

```bash
auberge ansible run --tags grimmory
```

## Configuration

| Setting  | Default       |
| -------- | ------------- |
| Port     | 6060          |
| Data     | /srv/grimmory |
| BookDrop | /srv/bookdrop |
| JVM      | Shenandoah GC |

## Features

- Multi-user with OIDC authentication
- EPUB, PDF, CBZ, CBR, CB7, MOBI, AZW, AZW3, and FB2 support
- Audiobook support (M4B, M4A, MP3, OPUS)
- Kobo and KOReader sync
- OPDS catalog
- BookDrop auto-import
- Auto metadata from Google Books, Open Library, and Goodreads

## Backup

Backed up by default. Includes:

- Book library
- Metadata database

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
