# ADR-0034: The extracted assets tree is addressed by its fingerprint, never rewritten in place

## Status

Accepted, 2026-08-24. **Extends ADR-0027's thesis one layer out** — a version is read from the artifact, and now the artifact's identity _is_ its version: the tree's directory name is its content fingerprint, so no two versions can occupy one path.

## Decision

The embedded `ansible/` tree extracts to `~/.local/share/auberge/ansible/<version>+<content-hash>/`, one directory per fingerprint, and nothing ever writes into a tree that already exists.

```
~/.local/share/auberge/ansible/
├── .lock                              # exclusive, held across extract-then-sweep
├── 0.15.15+c372d5766de336fa/          # one immutable tree per fingerprint
│   └── .lock                          # shared, held for as long as the tree is in use
└── collections/ae9381130640b5f2/      # ansible-galaxy cache, keyed on requirements.yml
```

Three rules make that layout safe under concurrency:

- **A tree appears atomically.** Extraction writes a `.staging*` directory beside the trees and `rename`s it into place, its `.lock` file already inside. A concurrent process sees the tree complete or not at all — never mid-write.
- **A tree in use cannot be swept.** Every `AnsibleAssets` value holds a shared `flock` on its tree's `.lock` for its whole lifetime, which spans the `ansible-playbook` child it was created for. The sweep removes a sibling only when it can take that lock _exclusively_, so liveness is measured, not estimated from an mtime.
- **Extraction and sweeping are serialized** by an exclusive lock on the container. Without it a tree could be swept in the window between its `rename` and its creator's first lock.

The sweep is opportunistic — it runs on every open, removes what it recognises as unused (an unlocked sibling tree, a crashed run's staging directory, the pre-#628 flat layout), and leaves anything else in the directory alone.

The galaxy cache stays shared, and is keyed on `requirements.yml` content for the same reason the trees are keyed on theirs: a changed requirement now lands in a sibling directory instead of deleting the cache a concurrent run is installing from.

## Why

`auberge deploy` (baikal → auberge, 2026-08-23) died mid-infrastructure on `'headscale_binary_path' is undefined` — a variable that does not exist in the v0.15.5 tree whose banner the run had printed, and that is _defined in role defaults_ in the v0.15.15 tree whose stamp was on disk afterwards. Neither tree can produce that error. Two can.

`ensure_extracted` cleared and re-extracted one shared path whenever the embedded fingerprint differed from the stamp, with no regard for what else was running. A stale mise pin ran v0.15.5; `mise up` installed v0.15.15 mid-play, and the next v0.15.15 invocation swapped the tree under the running play. **ansible-playbook compiles task lists and role defaults at play start but reads templates, `include_tasks`, `include_role`, `vars_files` and handler includes lazily, at task runtime.** So v0.15.5's compiled play rendered v0.15.15's `headscale.service.j2` — whose `ExecStart` names `headscale_binary_path`, while v0.15.5's hardcodes `/usr/local/bin/headscale` — against v0.15.5's variables.

The crash is the cheap half. `changed=10`: before dying, the run applied v0.15.5-era config to a host converged on ≥v0.15.14. A green run in the same race writes the same silent mix and reports success. The trigger was a stale pin, but the hazard needs no pin — any second invocation after an upgrade will do, and `AnsibleAssets::prepare()` is called by `config get`, `versions`, `backup`, `deploy` and the dependency resolver.

Naming a tree after its content removes the shared mutable path that made the swap expressible. What remains is deletion, and deletion is the one operation a lock can decide precisely.

## Considered alternatives

- **One `flock` on the existing shared directory** — exclusive to extract, shared for the whole playbook run. The issue's stated fallback, and it fixes the observed crash. Rejected as the primary: it keeps a single mutable path, so the swap stays _possible_ and merely becomes _excluded_ — every future reader of that directory, including a pre-#628 binary or an operator's `ansible-playbook` invocation, has to cooperate to preserve the invariant. Fingerprint addressing makes the invariant structural; the locks then only decide garbage collection, which is a strictly smaller thing to get right.
- **Age-based sweeping** — bump the tree's mtime on every open, delete siblings untouched for _N_ days. No lock file, no container lock, much less code. Rejected: it answers "when did a process last _start_ with this fingerprint", not "is one running now", and a wrong answer deletes the tree under a live play — the failure this ADR exists to remove, reintroduced in the cleanup path. `flock` answers the question that is actually being asked, and the standard library has had it since 1.89.
- **No sweeping at all.** Trees are 1.2 MB; a year of releases is under 50 MB. Genuinely defensible, and it would have shipped less code. Rejected because the pre-#628 flat layout is a 1.2 MB directory that would then sit in every operator's data dir forever, and because "the sweep is safe" is the property worth having tested — not the property worth deferring until the directory is noticed.
- **Extract per-run into a temp directory.** Perfect isolation, no shared state, no locks. Rejected on cost: ~2,000 files per invocation, and `prepare()` is called several times per command. It would also lose the galaxy cache, or reintroduce a shared path to hold it.
- **Keep the galaxy cache inside each tree.** Simpler — one directory, one lifetime, nothing shared. Rejected: a release cadence of days would re-download `community.general` per bump, and the cache is already content-keyed on `requirements.yml`, which is what makes sharing it safe.

## Consequences

**Positive:**

- A process reads one tree for its whole life. The mixed-version write is not merely unlikely; there is no path that expresses it.
- The forensic trail improves: the banner, the directory name and the `ansible.cfg` inside it all name the same fingerprint, and an old tree survives on disk instead of being overwritten by the run that failed.
- `.auberge-version` and the clear-then-extract path are gone. The stamp existed to answer "is this tree current"; the directory name answers it without a file to read, and without a window where the answer is wrong.
- An operator can pin an old auberge and a new one alternately with no re-extraction cost either way — both trees exist.

**Negative:**

- The galaxy cache moves from `ansible/.ansible/collections` to `ansible/collections/<hash>`, so the first run after this change re-installs collections once.
- A pre-#628 binary run after this change re-creates the flat layout beside the trees, and the next current-binary run sweeps it again. The transition converges but is not idempotent while both binaries are in play — the same shape as the incident, and unfixable from this side: the old binary takes no lock.
- Disk grows by one 1.2 MB tree per fingerprint that is live when a sweep runs. Bounded by how many auberge processes overlap, in practice one.
- A call site that discards its `AnsibleAssets` and keeps only a path — `dependency_resolver`, `inventory`, `recipe`, all of which read a file immediately — is unprotected for that window. The worst outcome there is a missing file and a hard error, not a crossed tree; a long-lived reader (`ansible-playbook`) always holds its guard for the duration.
- Two concurrent first-time galaxy installs into the same keyed directory can still interleave. Pre-existing, unchanged by this decision, and out of scope for #628.

## References

- Issue #628 — the incident, with the fingerprint reconstruction that proved two trees were involved.
- ADR-0027 — an installed version is read from the artifact, never from a note the role wrote. The stamp file this decision deletes was exactly such a note.
- ADR-0024 — a host rename recovers by rerun. Same remedy for the 10 mixed-version changes: re-run `auberge deploy` with the current binary.
