# TGTG Bot

Telegram bot that monitors Too Good To Go for available food bags and sends notifications. Docs: [github.com/TorbenStriegel/TooGoodToGo-TelegramBot](https://github.com/TorbenStriegel/TooGoodToGo-TelegramBot)

- **URL**: no public URL — outbound polling only, no inbound port
- **Data**: `/var/lib/tgtg/` on the VPS

## Deploy

```bash
auberge deploy tgtg
```

## Required config

| Key                       | Purpose                                             |
| ------------------------- | --------------------------------------------------- |
| `tgtg_telegram_bot_token` | Bot token from [@BotFather](https://t.me/BotFather) |

## Notes

?> After deploy, log in to TGTG by messaging your bot: `/login email@example.com`, then `/pin 12345` with the PIN sent to your email.

Runs as a dedicated `tgtg` system user with systemd hardening (`NoNewPrivileges`, `PrivateTmp`, `ProtectSystem=strict`). Bot token stored in `config.ini` (mode `0600`).
