# Three small utilities, not a god-runner, for command orchestration

Commands share a few interactive concerns (host selection, confirmation prompts, Ctrl-C handling), captured as three independent small utilities — `host::select_or_arg`, `prompt::confirm`, `signal::with_ctrlc` — not as a single `RunWithConfirmation` trait or builder that wraps the whole command lifecycle.

## Considered Options

- **`RunWithConfirmation` god-runner** (the original architecture-review suggestion). Rejected: the four command surfaces with overlapping shape — `ansible`, `headscale`, `dns`, `backup` — are _not_ the same shape underneath. Ansible runs playbooks via a subprocess; headscale wraps a remote CLI over SSH; dns calls the Cloudflare HTTP API; backup orchestrates Backup Recipes through a Backup Session. A single trait either has too few hooks (rigid) or too many optional fields (a builder with eight `Option`s, which is just scattered helpers wearing a coat). Forcing uniformity hides real differences.

## Why this is non-obvious

The "lift a shared command-runner" refactor is the textbook move and was suggested in the architecture review. This ADR records that the refactor was considered, that the commands' shapes were inspected, and that the deepening that actually earned its keep was three independent utilities — each passing the deletion test on its own — not a unifying abstraction. The most valuable of the three is `signal::with_ctrlc`, which fixes a known bug class (dangling progress bars on cancellation).
