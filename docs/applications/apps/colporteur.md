# Colporteur

Newsletter-to-feed converter: reads IMAP mailboxes, strips tracking pixels, and serves Atom XML feeds via HTTP basic auth. Repo: [github.com/sripwoud/colporteur](https://github.com/sripwoud/colporteur)

- **URL**: `https://feeds.{domain}` (basic auth)
- **Port**: internal (Caddy static file server)
- **Data**: `/var/lib/colporteur/feeds/`

## Deploy

```bash
auberge deploy colporteur
```

## Required config

Config lives locally at `~/.config/colporteur/config.toml` and is copied to the VPS on deploy. The file must exist before deploying.

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

## Notes

?> Set `colporteur_freshrss_sync = true` in `~/.config/auberge/config.toml` to auto-import feeds into [FreshRSS](freshrss.md) on each deploy. Requires FreshRSS on the same server.

Runs every 15 minutes via systemd timer. Check status:

```bash
sudo systemctl status colporteur.timer
sudo journalctl -u colporteur.service --since "1 hour ago"
```
