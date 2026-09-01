# ADR-0064: Agent memory pools in one directory; capture state and the index stay per-worktree

## Status

Accepted, 2026-09-01. Decided and implemented in #744. Implements the memory layer of [ADR-0054](./0054-agent-workloads-run-on-a-dedicated-disposable-host.md); narrows what #744 specified, for the reason below.

## Decision

**One directory is box-global: the memory directory.** `/srv/agent-memory/.memsearch/memory` holds every agent's dated markdown, whichever worktree wrote it, and it is the Syncthing folder that replicates to lechuck. Nothing else is shared:

| thing                               | scope        | why                                                                   |
| ----------------------------------- | ------------ | --------------------------------------------------------------------- |
| `memory/*.md`                       | box-global   | the durable state; the only thing that leaves the box                 |
| `.capture.pid`, `opencode-turns.db` | per-worktree | the daemon's single-instance lock and its per-session cursor          |
| Milvus collection                   | per-worktree | derived from the worktree path by the plugin, with no override        |
| `~/.memsearch/milvus.db`            | box-global   | one index file, many collections in it; rebuildable, never replicated |
| `agent_memory` collection           | box-global   | the operator's own index over the whole corpus, written by the deploy |

**#744 asked for a single box-global store _and collection_, pinned explicitly. The collection half is not reachable**, and pretending otherwise would have shipped a role that silently captured nothing. `@zilliz/memsearch-opencode` derives both the store path and the collection name from OpenCode's `worktree`, in the plugin's entry point:

```ts
const projectDir = (worktree && worktree !== "/") ? worktree : (directory || process.cwd());
const collectionName = deriveCollectionName(projectDir);
const memoryDir = join(projectDir, ".memsearch", "memory");
```

`MEMSEARCH_DIR` — the environment variable that pins a shared scope, honored by the claude-code, codex and openclaw plugins (upstream `9b003fc fix(openclaw): honor MEMSEARCH_DIR for storage scope`) — is not read here. Verified against the published `@zilliz/memsearch-opencode@0.3.15` tarball, not only against a checkout.

**So the sharing is done one level down, by symlinking `memory/` rather than `.memsearch/`.** Each worktree keeps its own `.memsearch/` and its own capture daemon; `<worktree>/.memsearch/memory` points at the box-global directory. Sharing the whole `.memsearch/` instead — the obvious move — collapses every agent's capture into one: `.capture.pid` is the daemon's single-instance lock (`capture-daemon.py`), so the second worktree's plugin finds a live PID, declines to start, and captures nothing. Every agent past the first would look healthy and remember nothing.

**Ansible writes Syncthing's configuration through Syncthing's REST API, not through `config.xml`.** The role previously appended a `<folder>` with `blockinfile`, keyed on an XML-comment marker. Syncthing re-serializes the whole document whenever it normalizes or migrates its config, and its serializer does not preserve comments — so the marker disappears, and the next deploy appends a second `<folder>` with the same id. Every write is guarded by a comparison against the running configuration, held there by `tests/memsearch_store_egress.rs`, because `PUT` answers 200 whether or not it changed anything. Three properties fall out of that and are fenced with it:

- **The comparison covers every field the write sends**, `paused` included. A field in the body but not in the check is one a human can change in the web UI and keep — and a paused folder that reads as converged stops replication while every later deploy reports success.
- **Device IDs are compared dash-stripped and upper-cased**, because Syncthing canonicalizes an ID on parse and the text pasted into `config.toml` is not necessarily the text it reports back.
- **The API block is skipped under `--check`.** `ansible.builtin.uri` declares no check mode, so the reads are skipped while the guards dereferencing them still run — which fails the play rather than reporting it, on `apps.yml` as much as here.

**The role asserts only what a caller declared.** `syncthing_discovery_enabled` defaults to unset, not to `true`, and the web-UI address is written one way only. Syncthing's own web UI is a second writer for both, so a role enforcing its defaults would undo a hand-hardened Host on its next `apps.yml` deploy. This is the one place the repo's declare-and-converge posture yields: the declaration is what a caller wrote, not what the role would prefer.

## Why

The acceptance that matters is _"an agent in one worktree can recall what an agent in another worktree learned."_ A shared corpus delivers that even with per-worktree collections, because each collection indexes the whole shared directory: agent A's memory is in agent B's collection because B indexed the file A wrote. A shared _collection_ would be an efficiency, not a capability.

Keeping the index on-box rather than on lechuck follows from the ACL: `tag:agent` reaches only `tag:data:53` ([ADR-0055](./0055-the-tailnet-runs-a-tag-based-acl-policy.md)), so an off-box index is unreachable from the box whose agents are the point. Embedding stays local ONNX (`bge-m3`) because embeddings see raw transcript text — an API embedder puts a third party deeper in the path than the model layer already does, on a Host assumed compromisable.

The folder is `sendreceive` rather than `sendonly`. Send-only is the intuitive direction for egress and it is the wrong one: a rebuilt box starts with an empty folder, and a send-only peer propagates that emptiness as deletions. A `sendreceive` folder with no local index pulls instead, which is what "box rebuild loses the index and no memories" requires.

## What it costs

**Embeddings are computed once per worktree, not once.** N worktrees means N collections over the same corpus in one `milvus.db`. On a box budgeted at 8 GB with 3–6 concurrent agents this is the cost worth watching, and it is the one that disappears if the plugin ever gains `MEMSEARCH_DIR` parity — at which point this ADR is amended, not rewritten, because the directory layout already assumes a shared corpus.

**Concurrent daemons append to one daily markdown file.** Each writes a summary of a few hundred bytes with a single `open(..., "a")`; Python may split a large write across `write()` calls, so interleaving is possible rather than impossible. Not mitigated: the anchors that make an entry addressable (`<!-- session:… turn:… -->`) are per-turn, so a torn entry costs one summary and corrupts no other.

**A compromised agent can delete the corpus, and Syncthing will replicate the deletion.** Inherent to sync, not to this design; the mitigation is file versioning on lechuck's side of the folder, which this repo does not manage.

**Two things must land before capture actually runs.** The plugin declaration in OpenCode's config belongs to #741, which owns that file, and the per-worktree `memory` symlink belongs to #740, which creates the worktrees. The role publishes `memsearch_memory_dir` as the symlink target for both.

## Alternatives considered

- **Share the whole `.memsearch/` directory.** Rejected above: one PID file, one daemon, silent capture loss for every agent but the first.
- **Patch `MEMSEARCH_DIR` support into the plugin upstream first.** The correct long-term fix, and it needs more than `MEMSEARCH_DIR`: the daemon's state (`pid`, `opencode-turns.db`) must stay per-project while the memory directory and collection go shared, which is a split the other plugins do not have to make because none of them run a daemon. Rejected as a blocker — it puts a third-party review cycle in front of a box that is already paid for.
- **Vendor a patched plugin in the role's `files/`.** Rejected: auberge would own a fork of a third-party plugin, tracked against upstream drift, for an efficiency rather than a capability.
- **Index on lechuck instead of on-box.** Rejected by the ACL: `tag:agent` cannot query it. If memory search ever becomes purely operator-facing, moving it deletes the RAM cost entirely — an amendment to ADR-0054, not to this.
- **Put the store under the admin user's `~/.memsearch/`.** Rejected: that directory holds `milvus.db`, and a Syncthing folder over it would replicate a file memsearch holds open. The store and the index are separated precisely so the folder can be pure markdown.
- **Keep `blockinfile` for the Syncthing folder.** Rejected: it works exactly once, and its second run corrupts the config it wrote.
