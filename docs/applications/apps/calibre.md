# Calibre

Calibre-Web is a lightweight alternative to the default [Booklore](booklore.md) book library. It uses significantly less RAM (~50MB vs ~1GB) and doesn't require MariaDB or a JVM.

Ebook library management and web interface for reading and organizing books.

Official Documentation: [https://calibre-ebook.com](https://calibre-ebook.com)

## Deployment

```bash
auberge ansible run --playbook playbooks/calibre.yml
```

## Backup

Backed up by default. Includes:

- Book library
- Metadata database
- User database (login credentials)

See [Backup & Restore](../../backup-restore/overview.md).

## Related

- [Backup & Restore](../../backup-restore/overview.md)
- [Applications Overview](../overview.md)
