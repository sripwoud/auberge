# Blocky

Recursive DNS resolver with built-in ad/tracker blocking. [Upstream docs](https://0xerr0r.github.io/blocky).

- **URL**: `blocky.{domain}` (DoT only, port 853)
- **Internal**: port 53 on Tailscale IP (tailnet DNS), port 5353 (general)

## Deploy

```bash
auberge ansible run --tags blocky
```

## Required config

| Key                        | Purpose                                                                                              |
| -------------------------- | ---------------------------------------------------------------------------------------------------- |
| `tailscale_api_key`        | Optional. When set, Blocky auto-registers itself as the tailnet DNS nameserver and enables MagicDNS. |
| `cloudflare_dns_api_token` | Required. Provisions the public `blocky.{domain}` A record.                                          |

!> Public exposure is **DoT only** (853/tcp). Plain 53/udp is bound to the Tailscale interface.

## Off-tailnet DoT clients

Resolver hostname: `blocky.{domain}` on port 853.

<details>
<summary>iOS</summary>

iOS has no native DoT UI. Generate a `.mobileconfig` profile referencing `tls://blocky.{domain}:853` (via [Apple Configurator 2](https://apps.apple.com/app/apple-configurator-2/id1037126344) or [encrypted-dns](https://github.com/paulmillr/encrypted-dns)), AirDrop it, then enable under **Settings → General → VPN & Device Management → DNS**.

</details>

<details>
<summary>Android (9+)</summary>

**Settings → Network & internet → Private DNS → Private DNS provider hostname** → `blocky.{domain}`.

</details>

<details>
<summary>macOS</summary>

Either install a `.mobileconfig` profile (same shape as iOS, double-click to install via **System Preferences → Profiles**), or use [`dnscrypt-proxy`](https://github.com/DNSCrypt/dnscrypt-proxy) with a DoT stamp generated at [dnscrypt.info/stamps](https://dnscrypt.info/stamps) (protocol=DoT, host=`blocky.{domain}`, port=853), then point macOS DNS at `127.0.0.1`.

</details>
