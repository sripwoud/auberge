# memsearch

Semantic memory for the agent Host: agents accumulate memory across sessions and across worktrees, and it survives the box. Docs: [github.com/zilliztech/memsearch](https://github.com/zilliztech/memsearch). Decision record: [ADR-0064](../../../meta/adr/0064-agent-memory-pools-in-one-directory.md).

- **URL**: none; a CLI and a library, not a service
- **Data**: `/srv/agent-memory/.memsearch/memory` (markdown, replicated off-box), `~/.memsearch/milvus.db` (index, disposable)
- **Backup**: none, by decision — the agent Host holds no state a backup is the answer for ([ADR-0054](../../../meta/adr/0054-agent-workloads-run-on-a-dedicated-disposable-host.md))

## Deploy

```bash
auberge ansible run --tags memsearch -H ruche
```

## Configuration

| Key                          | Description                                                 |
| ---------------------------- | ----------------------------------------------------------- |
| `memsearch_sync_device_id`   | Syncthing device ID of the peer that holds the off-box copy |
| `memsearch_sync_device_name` | How that peer appears in the Host's device list             |

The peer initiates the connection. The Host announces nowhere, so pair it from the peer's side using the Host's tailnet address and port 22000.

## Notes

The store is markdown and the index is rebuilt from it, so losing the box loses the index and nothing else:

```bash
rm -f ~/.memsearch/milvus.db
memsearch index /srv/agent-memory/.memsearch/memory
```

Each agent worktree keeps its own capture daemon and its own collection over that one shared corpus — an agent in one worktree therefore recalls what an agent in another learned. The reason it is not one collection, and the cost of that, are in ADR-0064.
