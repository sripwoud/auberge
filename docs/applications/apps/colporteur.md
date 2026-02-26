# Colporteur

Newsletter-to-feed converter. Connects to IMAP mailboxes, fetches newsletter emails, and generates Atom XML feeds.

Repository: [https://github.com/sripwoud/colporteur](https://github.com/sripwoud/colporteur)

## Deployment

```bash
auberge ansible run --tags colporteur
```

## How It Works

Colporteur runs as a systemd timer (every 15 minutes by default):

1. Connects to configured IMAP accounts
2. Searches for unread emails from configured senders
3. Parses and sanitizes HTML content (strips tracking pixels)
4. Generates Atom XML feed files
5. Caddy serves the static XML files

Feeds are consumed by [FreshRSS](freshrss.md) or any RSS reader via their static URLs.

## Configuration

### Environment Variables

| Variable                             | Description                               |
| ------------------------------------ | ----------------------------------------- |
| `COLPORTEUR_SUBDOMAIN`               | Subdomain for feed serving (e.g. `feeds`) |
| `COLPORTEUR_IMAP_<ACCOUNT>_PASSWORD` | IMAP password per account (age-encrypted) |

### Accounts and Feeds

Accounts and feeds are defined via `colporteur_accounts` and `colporteur_feeds` Ansible variables:

```yaml
colporteur_accounts:
  mxroute1:
    server: "mail.mxroute.com"
    username: "user@domain1.com"
    password: "{{ lookup('env', 'COLPORTEUR_IMAP_MXROUTE1_PASSWORD') }}"
  mxroute2:
    server: "mail.mxroute.com"
    username: "news@domain2.com"
    password: "{{ lookup('env', 'COLPORTEUR_IMAP_MXROUTE2_PASSWORD') }}"

colporteur_feeds:
  my-newsletter:
    title: "My Newsletter"
    account: "mxroute1"
    senders: ["hello@newsletter.com"]
```

## Operations

```bash
sudo systemctl status colporteur.timer
sudo journalctl -u colporteur.service --since "1 hour ago"
```

## Related

- [FreshRSS](freshrss.md)
- [Applications Overview](../overview.md)
