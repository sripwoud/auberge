# Blocky

DNS server with integrated ad and tracking blocking capabilities.

Official Documentation: [https://0xerr0r.github.io/blocky](https://0xerr0r.github.io/blocky)

## Deployment

```bash
auberge ansible run --tags blocky
```

## Configuration

Auberge configures Blocky to:

- Provide DNS resolution with ad-blocking
- Support DNS over TLS (DoT) on port 853
- Listen on port 53 on the Tailscale interface for tailnet DNS queries
- Listen on port 5353 for general DNS queries
- Use upstream DNS resolvers (Cloudflare, Google)
- Publish a public Cloudflare A record at `blocky.<domain>` pointing at the host's public IPv4

## Tailscale DNS Integration

When `tailscale_api_key` is set in `config.toml`, Blocky automatically registers itself as the tailnet DNS nameserver via the Tailscale API. This means all devices on the tailnet get ad-blocking DNS without manual configuration.

The deployment sequence:

1. Blocky binds to port 53 on the server's Tailscale IP
2. Blocky waits until the port is listening
3. Blocky calls the Tailscale API to set itself as the DNS nameserver
4. MagicDNS is enabled to activate DNS override for all tailnet clients

See [Environment Variables](../../configuration/environment-variables.md#tailscale-api-key) for API key setup.

## Off-Tailnet DNS over TLS (DoT) Setup

Blocky is reachable as a DNS-over-TLS (DoT) resolver from anywhere on the internet — no Tailscale required. Use this to get ad-blocking and family-safe filtering on any device.

**Resolver hostname:** `blocky.<domain>` (port 853)

> **Note:** Only DoT (port 853) is exposed publicly. Plain UDP/TCP port 53 is restricted to the tailnet interface.

### iOS

iOS has no native UI to add arbitrary DoT servers — encrypted DNS is set via configuration profiles only.

1. Generate a `.mobileconfig` profile referencing `tls://blocky.<domain>` on port 853 (e.g. via [Apple Configurator 2](https://apps.apple.com/app/apple-configurator-2/id1037126344) or [encrypted-dns](https://github.com/paulmillr/encrypted-dns)).
2. AirDrop or email the file to the device, open it, and tap **Allow** to download the profile.
3. Go to **Settings → General → VPN & Device Management → DNS** → select the installed profile → enable it.

### Android

Android 9 and later has a built-in "Private DNS" setting:

1. Go to **Settings → Network & internet → Private DNS**.
2. Select **Private DNS provider hostname**.
3. Enter: `blocky.<domain>`.
4. Tap **Save**.

### macOS

macOS does not have native DoT support in System Preferences. Use a DNS profile or a third-party tool:

**Option A — DNS configuration profile (recommended)**

1. Create a `.mobileconfig` profile referencing `tls://blocky.<domain>` on port 853.
2. Install the profile via **System Preferences → Profiles** (double-click the `.mobileconfig` file).

**Option B — [dnscrypt-proxy](https://github.com/DNSCrypt/dnscrypt-proxy)**

1. Install via Homebrew: `brew install dnscrypt-proxy`.
2. In `/usr/local/etc/dnscrypt-proxy/dnscrypt-proxy.toml`, set:
   ```toml
   listen_addresses = ['127.0.0.1:53']
   server_names = ['blocky']

   [static.'blocky']
   stamp = 'sdns://...'  # generate at https://dnscrypt.info/stamps with protocol=DoT, host=blocky.<domain>, port=853
   ```
3. Start the service: `sudo brew services start dnscrypt-proxy`.
4. Point macOS DNS at `127.0.0.1` (**System Settings → Network → Wi-Fi → Details → DNS**).

## Related

- [Tailscale](./tailscale.md)
- [DNS Management](../../dns/managing-records.md)
- [Applications Overview](../overview.md)
