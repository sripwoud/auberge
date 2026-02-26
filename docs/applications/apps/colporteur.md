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

Config is managed locally at `~/.config/colporteur/config.toml` and copied to the VPS on deploy. The file must exist locally before running `auberge ansible run --tags colporteur`.

Example config:

```toml
output_dir = "/var/lib/colporteur/feeds"

[accounts.mxroute]
server = "mail.mxroute.com"
username = "user@domain.com"
password = "secret"

[feeds.my-newsletter]
title = "My Newsletter"
account = "mxroute"
senders = ["hello@newsletter.com"]
```

## Operations

```bash
sudo systemctl status colporteur.timer
sudo journalctl -u colporteur.service --since "1 hour ago"
```

## Related

- [FreshRSS](freshrss.md)
- [Applications Overview](../overview.md)
