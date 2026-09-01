# memsearch Role

Semantic memory for the agent Host: a box-global markdown store, an on-box vector index over it, and the CLI both are reached through. Decision record: [ADR-0064](../../../meta/adr/0064-agent-memory-pools-in-one-directory.md).

## What it installs

- `uv` under the target user's home (a Tool Version, pinned in defaults)
- `memsearch[onnx]` as a `uv` tool, pinned to the App Version in `memsearch.meta.yml`
- `/usr/local/bin/memsearch`, a symlink, so `which memsearch` answers for any process regardless of how its PATH was assembled — which is what the OpenCode plugin's CLI detection does before falling back to an unpinned `uvx`
- `~/.memsearch/config.toml`, pinning the index location, the operator's collection, and local ONNX embedding (no API key)
- `/srv/agent-memory/.memsearch/memory`, the box-global store

The last deploy step runs `memsearch index` over the store. On a fresh box that downloads the ONNX model (~560 MB, once) and so proves the embedding path works with no API key; afterwards it is incremental and keeps the operator's `agent_memory` collection current.

## What it does not do

Two seams belong to the roles that own the files:

| seam                                                            | owner                               |
| --------------------------------------------------------------- | ----------------------------------- |
| `"plugin": ["@zilliz/memsearch-opencode"]` in OpenCode's config | the `opencode` role (#741)          |
| `<worktree>/.memsearch/memory -> {{ memsearch_memory_dir }}`    | the `aoe` role (#740), per worktree |

`memsearch_memory_dir` is published as a role default for exactly that reason: it is the symlink target, and it is the path the Syncthing folder replicates.

**Symlink `memory/`, never `.memsearch/`.** The capture daemon's single-instance lock is `<worktree>/.memsearch/.capture.pid`; sharing the parent directory means the second worktree's daemon finds a live PID, declines to start, and captures nothing while looking healthy.

## Variables

| Variable                    | Default                               | Description                                              |
| --------------------------- | ------------------------------------- | -------------------------------------------------------- |
| `memsearch_user`            | `admin_user_name` / `ansible_user`    | User the tool is installed for and the store is owned by |
| `memsearch_store_dir`       | `/srv/agent-memory/.memsearch`        | Box-global store root                                    |
| `memsearch_memory_dir`      | `{{ memsearch_store_dir }}/memory`    | The dated markdown; the Syncthing folder                 |
| `memsearch_state_dir`       | `~/.memsearch`                        | Config and index; never replicated                       |
| `memsearch_index_uri`       | `{{ memsearch_state_dir }}/milvus.db` | Milvus Lite file                                         |
| `memsearch_collection`      | `agent_memory`                        | The operator's collection over the whole corpus          |
| `memsearch_embedding_model` | `gpahal/bge-m3-onnx-int8`             | Local ONNX model; no API key                             |
| `memsearch_uv_version`      | pinned in defaults                    | Tool Version, renovate-annotated                         |

## Rebuilding the index

The index is disposable by construction; the markdown is not.

```bash
rm -f ~/.memsearch/milvus.db
memsearch index /srv/agent-memory/.memsearch/memory
memsearch search "<something an agent learned>"
```

## Tags

- `memsearch`, `memory`
