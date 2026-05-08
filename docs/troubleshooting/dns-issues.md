# DNS Issues

## Authentication & permissions

| Error                        | Fix                                                                                                                               |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `"Authentication error"`     | Regenerate token in Cloudflare Dashboard; `auberge config set cloudflare_dns_api_token <token>`; verify with `auberge dns status` |
| `"Insufficient permissions"` | Recreate token with **DNS Edit** + **Zone Read** permissions                                                                      |
| `"Zone not found"`           | `auberge config get domain`; `auberge config set domain example.com`; confirm domain exists in Cloudflare                         |
| `"Multiple zones found"`     | Scope token to a specific zone                                                                                                    |
| `"DNS-01 challenge failed"`  | Token invalid or missing DNS edit permission — verify with `auberge dns status`                                                   |

## Record operations

| Error                     | Cause                                    | Fix                                                                  |
| ------------------------- | ---------------------------------------- | -------------------------------------------------------------------- |
| `"Record already exists"` | N/A — commands are idempotent            | Safe to retry; record updates in place                               |
| `"Invalid IP address"`    | Non-IPv4 value (hostname, IPv6, IP:port) | `auberge dns set --subdomain cal --ip 203.0.113.10`                  |
| `"Rate limit exceeded"`   | Rapid repeated calls                     | Wait 60s; prefer `auberge dns set-all` over individual sets in loops |

## Propagation

New record not resolving or old IP still showing — work through in order:

```bash
# 1. Query Cloudflare's authoritative resolver directly
dig @1.1.1.1 subdomain.example.com +short

# 2. Flush local cache
sudo systemd-resolve --flush-caches   # Linux
sudo dscacheutil -flushcache          # macOS

# 3. Confirm record state
auberge dns list --subdomain cal
```

?> Default TTL is 5 minutes. Wait before concluding the record is broken.

## Migration

### `"Some records failed to migrate"`

Re-run — `dns migrate` is idempotent:

```bash
auberge dns migrate --ip 10.0.0.1
```

### Unexpected records migrated

`dns migrate` updates **all** records pointing at the old IP. Use `dns set` to target specific subdomains instead:

```bash
auberge dns set --subdomain cal --ip 10.0.0.1
auberge dns set --subdomain rss --ip 10.0.0.1
```

!> CGNAT addresses (100.64.0.0/10) are skipped by `dns migrate` — use `dns set-all` explicitly for Tailscale IPs.

## Batch operations

| Error                                        | Fix                                                                                                                                  |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `"No subdomain environment variables found"` | `auberge config list \| grep subdomain` — set missing `*_subdomain` keys                                                             |
| DNS records exist but apps unreachable       | Deploy apps (`auberge ansible run --host vps --tags apps`); check `sudo ufw status` and `sudo systemctl status caddy`                |
| App deployed but subdomain missing           | Some roles (Blocky, Calibre) don't include `dns_record` integration — create manually: `auberge dns set --subdomain <sub> --ip <ip>` |

## Verification

```bash
auberge dns list                          # all records
dig subdomain.example.com +short          # live resolution
dig @8.8.8.8 subdomain.example.com        # bypass local cache
```
