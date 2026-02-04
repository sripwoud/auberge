# OpenClaw Deployment

OpenClaw is a personal AI assistant that runs on your VPS and connects to messaging platforms (WhatsApp, Telegram, Slack, Discord, etc.).

## Architecture

```
Your Phone (WhatsApp/Telegram)
    ↓ (outbound HTTPS to messaging APIs)
Your VPS (OpenClaw Gateway)
    ↓ (binds to Tailscale IP)
Your Laptop (Web UI access via Tailscale)
```

## Prerequisites

1. **Bootstrap layer** must be run first (users, SSH, firewall)
2. **Infrastructure layer** must include Tailscale (`infrastructure.yml`)
3. **Environment variables** must be set (see Configuration below)

## Configuration

### Required Environment Variables

Add to `mise.toml` in auberge root:

```toml
[env]
OPENCLAW_GATEWAY_TOKEN = "generate-secure-token-here"
CLAUDE_AI_SESSION_KEY = "sk-ant-..."
```

Optional variables:

```toml
CLAUDE_WEB_SESSION_KEY = "..."
CLAUDE_WEB_COOKIE = "..."
```

### Generate Gateway Token

```bash
openssl rand -hex 32
```

### Get Claude AI Session Key

1. Login to https://claude.ai
2. Open browser DevTools (F12)
3. Go to Application → Cookies
4. Copy `sessionKey` value

## Deployment

### Deploy OpenClaw Only

```bash
cd /home/sripwoud/code/auberge
ansible-playbook ansible/playbooks/openclaw.yml -i ansible/inventory.yml
```

### Deploy with Other Layers

```bash
ansible-playbook ansible/playbooks/bootstrap.yml -i ansible/inventory.yml
ansible-playbook ansible/playbooks/infrastructure.yml -i ansible/inventory.yml
ansible-playbook ansible/playbooks/openclaw.yml -i ansible/inventory.yml
```

### Check Mode (Dry Run)

```bash
ansible-playbook ansible/playbooks/openclaw.yml -i ansible/inventory.yml --check
```

## Post-Deployment Setup

### 1. SSH to VPS

```bash
ssh user@your-vps
```

### 2. Run Onboarding Wizard

```bash
openclaw onboard
```

The wizard will guide you through:

- Connecting messaging channels (WhatsApp, Telegram, etc.)
- Configuring AI model preferences
- Setting up skills and workspace

### 3. Connect Messaging Platforms

**WhatsApp:**

- Wizard will show QR code
- Scan with WhatsApp on your phone
- OpenClaw connects via Baileys library

**Telegram:**

1. Create bot via @BotFather
2. Copy bot token
3. Provide to wizard

**Slack:**

1. Create bot in Slack workspace
2. Copy bot token
3. Provide to wizard

**Discord:**

1. Create bot in Discord Developer Portal
2. Copy bot token
3. Provide to wizard

### 4. Access Web UI

**Install Tailscale on laptop:**

```bash
# macOS/Linux
https://tailscale.com/download

# Login with same account as VPS
tailscale login
```

**Access gateway:**

```bash
# Find VPS Tailscale IP
ssh user@your-vps
tailscale ip -4

# Browse to (from laptop with Tailscale)
http://100.64.0.X:18789
```

## Service Management

### Check Service Status

```bash
systemctl --user status openclaw-gateway
```

### View Logs

```bash
journalctl --user -u openclaw-gateway -f
```

### Restart Service

```bash
systemctl --user restart openclaw-gateway
```

### Stop Service

```bash
systemctl --user stop openclaw-gateway
```

### Disable Service

```bash
systemctl --user disable openclaw-gateway
```

## Daily Usage

### Messaging Platforms

Once connected, just message openclaw via your preferred platform:

- **WhatsApp**: Send message to paired phone number
- **Telegram**: Message your bot
- **Slack**: @mention the bot in channels
- **Discord**: @mention the bot in channels

The AI responds in the same channel automatically.

### CLI Commands (via SSH)

```bash
ssh user@your-vps

openclaw message send --to +1234567890 --message "Hello"

openclaw agent --message "Ship checklist" --thinking high

openclaw doctor
```

## Security

### Network Exposure

- **Public Internet**: Only SSH (22) + Tailscale (41641/udp)
- **OpenClaw Ports**: NOT exposed publicly
- **Gateway Binding**: Tailscale IP only (100.64.0.X)
- **Access Method**: Via Tailscale VPN

### Firewall Rules (UFW)

```bash
22/tcp    ALLOW  # SSH
41641/udp ALLOW  # Tailscale
18789/tcp DENY   # OpenClaw gateway (Tailscale-only)
18790/tcp DENY   # OpenClaw bridge (Tailscale-only)
```

### Secret Management

- Environment variables via mise.toml
- Never commit .env files
- Rotate tokens regularly
- Use strong gateway token

## Troubleshooting

### Service Won't Start

```bash
journalctl --user -u openclaw-gateway -n 50
```

Check for:

- Missing environment variables
- Node.js version < 22
- Port conflicts

### Can't Access Web UI

1. Verify Tailscale running:
   ```bash
   tailscale status
   ```

2. Check gateway binding:
   ```bash
   ss -tlnp | grep 18789
   ```

3. Verify Tailscale IP:
   ```bash
   tailscale ip -4
   ```

### Messaging Platform Not Responding

```bash
openclaw doctor

journalctl --user -u openclaw-gateway -f
```

Check for:

- Session expired (re-run onboarding)
- Network connectivity
- API rate limits

## Updates

### Update OpenClaw

```bash
ssh user@your-vps
pnpm update -g openclaw
systemctl --user restart openclaw-gateway
```

### Update via Ansible

```bash
cd /home/sripwoud/code/auberge
ansible-playbook ansible/playbooks/openclaw.yml -i ansible/inventory.yml
```

## Removal

### Stop and Disable Service

```bash
ssh user@your-vps
systemctl --user stop openclaw-gateway
systemctl --user disable openclaw-gateway
```

### Uninstall OpenClaw

```bash
pnpm uninstall -g openclaw
rm -rf ~/.openclaw
rm ~/.config/systemd/user/openclaw-gateway.service
systemctl --user daemon-reload
```

## References

- [OpenClaw Docs](https://docs.openclaw.ai)
- [Getting Started](https://docs.openclaw.ai/start/getting-started)
- [Tailscale Setup](https://tailscale.com/kb/1017/install)
- [WhatsApp Integration](https://docs.openclaw.ai/channels/whatsapp)
- [Telegram Integration](https://docs.openclaw.ai/channels/telegram)
