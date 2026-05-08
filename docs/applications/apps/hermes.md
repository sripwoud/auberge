# Hermes

Self-improving personal AI assistant (Nous Research) that connects to Telegram and routes queries to your chosen LLM provider. Docs: [hermes-agent.nousresearch.com](https://hermes-agent.nousresearch.com/docs)

- **URL**: no public URL — outbound polling only, no inbound port
- **Data**: `~/.hermes/` on the VPS

## Deploy

```bash
auberge deploy hermes
```

Dependency layers (hardening, infrastructure) resolved automatically.

## Required config

| Key                         | Purpose                                                                              |
| --------------------------- | ------------------------------------------------------------------------------------ |
| `hermes_llm_provider`       | Provider slug: `anthropic`, `deepseek`, `gemini`, `kimi`, `openrouter`, `xai`, `zai` |
| `hermes_llm_api_key`        | API key for the chosen provider                                                      |
| `hermes_telegram_bot_token` | Bot token from [@BotFather](https://t.me/BotFather)                                  |

Optional: `hermes_exa_api_key` (web search), `hermes_telegram_allowed_users` (comma-separated user IDs — restricts bot access; without this any user knowing the token can interact).

## Notes

?> `hermes_llm_provider` must match the provider set in `~/.config/hermes/config.yaml`, which is synced to the VPS on deploy.

After deploy, verify the service and send a test message to your bot:

```bash
systemctl --user status hermes-gateway
journalctl --user -u hermes-gateway -f
```

Secrets stored in `~/.hermes/.env` (mode `0600`). No public ports exposed.
