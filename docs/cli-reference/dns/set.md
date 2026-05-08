# auberge dns set

Create or update a Cloudflare A record for a subdomain. Alias: `auberge d s`.

```bash
auberge dns set --subdomain <NAME> --ip <IP>
```

Upserts: updates the existing record if present, creates it otherwise. TTL is set to 1 (automatic). Record is DNS-only (not proxied).

## Options

| Option                 | Description          | Required |
| ---------------------- | -------------------- | -------- |
| `-s, --subdomain NAME` | Subdomain name       | Yes      |
| `-i, --ip IP`          | IPv4 or IPv6 address | Yes      |
| `-P, --production`     | Use production API   | No       |

## Examples

```bash
auberge dns set --subdomain freshrss --ip 192.168.1.10
auberge dns set --subdomain baikal --ip 10.0.0.5 --production
```

## Gotchas

- Invalid IP formats are rejected immediately.
- Cloudflare updates are instant but recursive resolvers may cache for up to the TTL. Verify with `dig freshrss.example.com`.
- For multiple subdomains use `auberge dns set-all --host myserver`.
