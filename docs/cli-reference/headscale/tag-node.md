# auberge headscale tag-node

Replace an enrolled node's ACL tags. Alias: `auberge hs tn`.

```bash
auberge headscale tag-node [NAME] -t TAG[,TAG...] [--host HOST]
```

## Description

ACL tags are stampable at pre-auth-key mint (`add-user -t`, `add-key -t`) and nowhere else, so a node that enrolled before those flags carried a tag has none — and a default-deny policy keyed on tags matches nobody. This is the only path that tags a node that is already in the tailnet.

The node is named, not numbered: the name is resolved against one `nodes list`, and only the numeric id it carries reaches headscale (`--identifier` is a `uint64`). Omitting the name opens a picker over that same listing; without a terminal it is an error, not a hang.

> [!WARNING]
> **This replaces the node's tag set — it does not add to it.** headscale's own `nodes tag --help` says "tags to add to the node", and that is wrong: `SetNodeTags` assigns the list wholesale, so a tag the node carries and your `-t` omits is dropped. auberge names any tag it drops on the way past.

> [!IMPORTANT]
> A node is owned by a user **or** by its tags, never both. Tagging a user-owned node clears its user, and headscale has no path back — the node then shows as owned by `tagged-devices` in `list-nodes`.

## Options

| Option        | Description                                       | Default                   |
| ------------- | ------------------------------------------------- | ------------------------- |
| `NAME`        | Node name as `list-nodes` shows it                | Interactive picker        |
| `-t, --tags`  | Tags the node ends up with (`tag:...`) — required | —                         |
| `--host HOST` | Target host running headscale                     | Serving host, else prompt |

## Examples

```bash
# Tag one node
auberge hs tn lechuck --tags tag:infra

# Several tags in one call
auberge hs tn ruche --tags tag:agent,tag:infra

# Pick the node from a listing
auberge hs tn --tags tag:infra
```

## Output

The resulting tag set, read off the mutation's own response — what the node ended up with, not what was asked for — plus a warning for each dropped tag and for a lost user owner.

No `--output`: [ADR-0004](https://github.com/sripwoud/auberge/blob/master/meta/adr/0004-cli-structured-output.md) puts the flag only on a command whose JSON carries a field the caller could not have predicted, and nothing consumes this one's. `auberge headscale list-nodes --output json` reads back what the fleet carries.

## Troubleshooting

**`requested tags [...] are invalid or not permitted`** — the tag is well-formed but the deployed ACL policy does not name it under `tagOwners`. `nodes tag` is the only path that checks: the `--tags` stamped on a pre-auth key is applied unchecked, which is why a node can already carry a tag no policy mentions. Add the tag to `policy.hujson`'s `tagOwners` and deploy before tagging.

**No node with that name** — `tag-node` never enrols nodes. Check the spelling against `auberge headscale list-nodes`, which is also where the `TAGS` column shows what each node currently carries.
