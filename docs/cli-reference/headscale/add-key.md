# auberge headscale add-key

Mint a pre-auth key for an existing user

## Synopsis

```bash
auberge headscale add-key [OPTIONS]
```

## Alias

`auberge hs ak`

## Description

Mints a pre-auth key for a user that already exists — the case `add-user` refuses. Enrolling a second device under the same user goes through here.

When run without `--user`, prompts with a picker over `list-users`. The pre-auth key can be passed to `tailscale up --authkey` to connect the node without manual approval.

## Options

| Option             | Description                                       | Default                   |
| ------------------ | ------------------------------------------------- | ------------------------- |
| `-u, --user`       | Existing username                                 | Interactive prompt        |
| `-e, --expiration` | Pre-auth key expiration (1h, 24h, 48h, 7d)        | 24h                       |
| `-t, --tags`       | ACL tags to stamp on the pre-auth key (`tag:...`) | None                      |
| `--host HOST`      | Target host running headscale                     | Serving host, else prompt |

## Examples

```bash
# Interactive mode: pick the user, then the expiration
auberge hs ak

# Mint a key for a laptop under an existing user
auberge hs ak --user sripwoud --expiration 1h

# Stamp the key so the enrolling node comes up tagged
auberge hs ak --user sripwoud --tags tag:server
```

## Output

Prints the pre-auth key and shareable setup instructions for iOS/Tailscale clients.

## Troubleshooting

**No user with that name**: `add-key` never creates users — run `auberge headscale add-user` first, or check the spelling against `auberge headscale list-users`.
