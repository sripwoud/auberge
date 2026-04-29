# Ladder

HTTP web proxy for testing and analyzing paywall implementations and content delivery on modern websites.

Repository: [https://github.com/everywall/ladder](https://github.com/everywall/ladder)

## Deployment

```bash
auberge deploy ladder
```

## How It Works

Ladder runs as a long-lived systemd service bound to `127.0.0.1:8086`. Caddy fronts it on `https://ladder.<domain>` with HTTP basic auth, proxying authenticated requests through to ladder. Ladder applies a ruleset (a YAML file that maps domain rules to header overrides, cookie injection, regex replacements, and DOM modifications) and forwards requests to the target site, returning a modified response.

The frontpage at `https://ladder.<domain>/` accepts a URL to proxy. You can also use a bookmarklet:

```javascript
javascript:window.location.href="https://ladder.<domain>/"+location.href
```

## Configuration

Required keys in `~/.config/auberge/config.toml`:

```toml
ladder_subdomain = "ladder"
ladder_basic_auth_password = "your-password"
```

The basic auth user is `admin_user_name` (already in the auberge config). Caddy hashes the password with `caddy hash-password` at deploy time — only the hash is written to disk in the Caddyfile.

## Ruleset

By default, ladder loads the upstream community ruleset on startup:

```
https://raw.githubusercontent.com/everywall/ladder-rules/main/ruleset.yaml
```

To use your own curated ruleset, set `ladder_ruleset_src` in the role vars to a local file path. The role copies it to `/etc/ladder/ruleset.yaml` and points `RULESET` at the local file. Ladder reloads the ruleset on service restart.

## FlareSolverr

The role does not deploy [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr). If you run FlareSolverr separately (or on another host), set `ladder_flaresolverr_host` (e.g. `http://localhost:8191`) in the role vars and ladder will route domains marked `useFlareSolverr: true` through it.

## Operations

```bash
sudo systemctl status ladder
sudo journalctl -u ladder --since "1 hour ago"
```

Service restarts pick up new rulesets and config changes.

## Related

- [Caddy](../infrastructure/caddy.md)
- [Applications Overview](../overview.md)
