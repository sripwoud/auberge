# Hermes Agent Deployment

Hermes Agent is a self-improving personal AI assistant by Nous Research. It connects to messaging platforms (Telegram, Discord, etc.) and learns from interactions via a closed learning loop.

## Architecture

```mermaid
flowchart TD
    A[Your Phone - Telegram] -->|outbound HTTPS polling| B[Your VPS - Hermes Gateway]
    B -->|outbound API calls| C[LLM Provider]
    C --> D[Kimi / Claude / DeepSeek / etc]
```

Hermes gateway connects **outbound** to Telegram's API. No inbound port exposure needed.

## Prerequisites

1. **Bootstrap layer** must be run first (users, SSH, firewall)
2. **Infrastructure layer** with Tailscale (for SSH access to VPS)
3. **API keys** set in config (see Configuration below)

## Configuration

### Required Config Values

```bash
auberge config set hermes_llm_provider <anthropic|deepseek|gemini|kimi|openrouter|xai|zai>
auberge config set hermes_llm_api_key <VALUE>
auberge config set hermes_telegram_bot_token <VALUE>
```

Supported providers:

| Provider           | Slug         | Env Var              | API Key Platform                                       |
| ------------------ | ------------ | -------------------- | ------------------------------------------------------ |
| Anthropic          | `anthropic`  | `ANTHROPIC_API_KEY`  | [console.anthropic.com](https://console.anthropic.com) |
| DeepSeek           | `deepseek`   | `DEEPSEEK_API_KEY`   | [platform.deepseek.com](https://platform.deepseek.com) |
| Google Gemini      | `gemini`     | `GOOGLE_API_KEY`     | [aistudio.google.dev](https://aistudio.google.dev)     |
| Kimi (Moonshot AI) | `kimi`       | `KIMI_API_KEY`       | [platform.moonshot.ai](https://platform.moonshot.ai)   |
| OpenRouter         | `openrouter` | `OPENROUTER_API_KEY` | [openrouter.ai/keys](https://openrouter.ai/keys)       |
| xAI (Grok)         | `xai`        | `XAI_API_KEY`        | [console.x.ai](https://console.x.ai)                   |
| Zhipu AI (GLM)     | `zai`        | `GLM_API_KEY`        | [open.bigmodel.cn](https://open.bigmodel.cn)           |

### Optional Config Values

```bash
auberge config set hermes_exa_api_key <VALUE>
auberge config set hermes_telegram_allowed_users <YOUR_TELEGRAM_USER_ID>
```

`hermes_telegram_allowed_users` restricts bot access to the specified Telegram user IDs (comma-separated). Without this, any Telegram user who knows the bot token can interact with it. Get your user ID by messaging @userinfobot on Telegram.

### Hermes Config

The LLM model and provider are configured in `~/.config/hermes/config.yaml`, which is synced to the VPS. The `hermes_llm_provider` in auberge config must match the provider set in `config.yaml`.

## Deployment

```bash
auberge deploy hermes
```

Dependency layers (hardening, infrastructure) are resolved and run automatically.

### Check Mode (Dry Run)

```bash
auberge deploy hermes --check
```

## Post-Deployment Setup

### 1. Verify Service

```bash
ssh user@your-vps
systemctl --user status hermes-gateway
```

### 2. Test Telegram Bot

Send a message to your bot on Telegram. It should respond.

## Service Management

### Check Status

```bash
systemctl --user status hermes-gateway
```

### View Logs

```bash
journalctl --user -u hermes-gateway -f
```

### Restart Service

```bash
systemctl --user restart hermes-gateway
```

### Stop Service

```bash
systemctl --user stop hermes-gateway
```

## Daily Usage

Message your Telegram bot. Hermes:

- Remembers context across sessions (SQLite FTS5)
- Creates reusable skills from complex tasks
- Supports slash commands (`/new`, `/model`, `/compress`, `/skills`)
- Transcribes voice memos
- Searches the web (with Exa API key)

## Security

- **No public ports**: Gateway polls Telegram outbound only
- **Secrets**: Stored in `~/.hermes/.env` with mode `0600`
- **User allowlist**: Set `hermes_telegram_allowed_users` to restrict bot access to specific Telegram user IDs
- **Command approval**: Hermes prompts before running dangerous commands (133 patterns)
- **Prompt injection scanning**: Detects hidden content and Unicode tricks

## Troubleshooting

### Service Won't Start

```bash
journalctl --user -u hermes-gateway -n 50
```

Check for:

- Missing API keys
- Python/uv not installed
- Network connectivity

### Bot Not Responding

```bash
journalctl --user -u hermes-gateway -f
```

Check for:

- Invalid Telegram bot token
- LLM API key out of credits or expired
- Rate limiting

## Updates

### Update via SSH

```bash
ssh user@your-vps
cd ~/.hermes/hermes-agent
git fetch && git checkout <new-tag>
VIRTUAL_ENV=~/.hermes/venv uv pip install -e ".[all]"
systemctl --user restart hermes-gateway
```

### Update via Ansible

```bash
auberge deploy hermes
```

## Removal

```bash
ssh user@your-vps
systemctl --user stop hermes-gateway
systemctl --user disable hermes-gateway
rm -rf ~/.hermes
rm -f ~/.local/bin/hermes
rm -f ~/.config/systemd/user/hermes-gateway.service
systemctl --user daemon-reload
```

## References

- [Hermes Agent Docs](https://hermes-agent.nousresearch.com/docs)
- [GitHub](https://github.com/NousResearch/hermes-agent)
- [Telegram BotFather](https://t.me/BotFather)
