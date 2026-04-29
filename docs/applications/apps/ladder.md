# Ladder

HTTP web proxy for testing and analyzing paywall implementations and content delivery on modern websites.

Repository: [https://github.com/everywall/ladder](https://github.com/everywall/ladder)

## Deployment

```bash
auberge deploy ladder
```

## How It Works

Ladder runs as a long-lived systemd service bound to `127.0.0.1:8086`. Caddy fronts it on `https://ladder.<domain>` with HTTP basic auth, proxying authenticated requests through to ladder. Ladder applies a ruleset (a YAML file that maps domain rules to header overrides, cookie injection, regex replacements, and DOM modifications) and forwards requests to the target site, returning a modified response.

## Usage

### Three ways to proxy a URL

- **Form**: open `https://ladder.<domain>/`, paste a URL, submit.
- **URL append**: navigate directly to `https://ladder.<domain>/<full-url>` (the target URL must include its scheme, e.g. `https://`).
- **Bookmarklet**: drop this in the bookmarks bar; clicking it on any page reloads through ladder.

```javascript
javascript:window.location.href="https://ladder.<domain>/"+location.href
```

### Authentication

The first request to `https://ladder.<domain>/` triggers Caddy's basic-auth dialog. Browsers cache credentials per-origin for the session, so subsequent clicks (including via the bookmarklet) are silent until the cache expires.

Do **not** bake credentials into the bookmarklet URL as `https://user:pass@ladder.<domain>/...` — Chrome and Edge silently strip the userinfo prefix from navigations (since 2018), and Firefox warns. Only Safari honors it. Rely on the browser's saved-password store instead.

### Verifying your deploy

- **Smoke test**: open `https://ladder.<domain>/https://example.com/`. After authenticating, you should see the standard "Example Domain" page rendered through ladder.
- **Ruleset coverage**: try a recent article from any site in the [upstream ruleset](https://github.com/everywall/ladder-rules) (Bloomberg, NYT, FT, Medium, Substack-paywalled posts).
- **What won't work**: sites behind Cloudflare's "Just a moment…" challenge. FlareSolverr is intentionally not bundled — see the FlareSolverr section below if you want to wire one up separately.

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
