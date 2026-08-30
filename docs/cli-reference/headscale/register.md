# auberge headscale register

Approve a pending interactive enrollment

## Synopsis

```bash
auberge headscale register <URL_OR_AUTH_ID> [OPTIONS]
```

## Alias

`auberge hs rg`

## Description

Interactive enrollment (the Android app, or `tailscale up` without `--authkey`) ends at a browser page instructing `headscale auth register --auth-id hskey-authreq-… --user USERNAME`. This subcommand runs that command over SSH — paste the page's URL instead of hand-copying the auth-id into an ssh one-liner.

Accepts the full register URL (`https://hs.…/register/hskey-authreq-…` — a trailing slash, query string, or fragment appended by a share sheet is tolerated) or the bare `hskey-authreq-…` id. Anything else is rejected locally, before any SSH round trip.

When run without `--user`, prompts with a picker over `list-users`.

## Options

| Option             | Description                               | Default            |
| ------------------ | ----------------------------------------- | ------------------ |
| `<URL_OR_AUTH_ID>` | Register URL or bare `hskey-authreq-…` id | Required           |
| `-u, --user`       | Existing username                         | Interactive prompt |
| `--host HOST`      | Target host running headscale             | Interactive prompt |

## Examples

```bash
# Paste the URL the enrolling device showed
auberge hs rg https://hs.example.com/register/hskey-authreq-x7K9m2P4qL8wN3vB5tR6yH1z

# Bare auth-id, user named explicitly
auberge hs rg hskey-authreq-x7K9m2P4qL8wN3vB5tR6yH1z --user sripwoud
```

## Output

Prints headscale's registration response and a success line naming the user the node came up under.

## Troubleshooting

**No pending enrollment under this auth-id**: headscale keeps a pending enrollment in an in-memory cache for 15 minutes, and drops all of them when it restarts, so an older link — or an id that was never issued — misses. Restart the login on the enrolling device and paste the fresh URL. The `/register/…` page keeps serving after the enrollment is gone; reloading it is not a liveness check.

**No user with that name**: `register` never creates users — run `auberge headscale add-user` first, or check the spelling against `auberge headscale list-users`.
