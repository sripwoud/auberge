# ADR-0047: A test seam is injected, never a compile-time `cfg`

## Status

Accepted, 2026-08-28.

## Decision

**A collaborator a test needs to substitute is a runtime argument.** `#[cfg(not(test))]` is not a seam. It makes the test binary and the shipped binary hold different code, so the production branch is the one branch no test can reach and the branch the tests reach is not the one that ships. `tests/seams_are_injected.rs` fails the build on any `not(test)` predicate under `src/`.

Two rules follow for `Progress`, the crate's one output seam:

- **A runner takes its Progress; it does not build one.** `&mut dyn Progress` where a runner reports about one thing (`RecipeExecutor::backup`, `AnsibleRunner::run`, `restic_push`, `restic_prune`); a factory, `Fn(&str) -> Box<dyn Progress>`, where it reports about many and the terminal shows a bar for each — `BackupSession::create` makes one per App, and a single injected Progress would collapse thirteen bars into one. Construction belongs to the command layer, the only layer allowed to know there is a terminal, and the same fence asserts every `TerminalProgress::` sits there.
- **Every message a runner emits is a Progress event.** An injected seam does not make output assertable on its own: a runner still reaching `eprintln!` or `output::*` has a path no `MockProgress` can see. The Backup Session had three, so the trait grew `success` (an item finished), `error` (an item failed and the run carried on) and `line` (a subprocess's own report, verbatim). `TerminalProgress` reproduces `output::success`/`info`/`warn` exactly, `--quiet` gate included, behind one `chrome` method — because gating two of those three is how `backup push`'s opening line escapes `--quiet`.

Render policy stays above the seam. Whether an App's result is streamed per-App or tabled at the end is the command's business: `ResultsSuppressed` in `commands/backup.rs` is a `Progress` that drops `success` and forwards the rest, so `create` emits one `success` per App and does not know what renders it.

The global output flags are untouched, and their teardown is not this decision. `is_verbose()` is read in four places, all in the command layer; `TerminalProgress` reads `is_quiet()` and `should_use_colors()` because it is the terminal. ADR-0004's `--output {human,json}` contract is a different surface and is not involved — this is how services reach stderr internally.

### What it costs

`ResultsSuppressed` forwards ten methods to drop one. A `bool` on `TerminalProgress` would be three lines and is rejected: it puts one command's render policy inside the shared renderer, where the next reader has to work out why `--verbose` makes a progress bar quieter. The forwarding is mechanical and one test asserts all ten events.

`success` and `error` disagree about colour — a green ✓, an uncoloured ✗. Each is exactly what it replaced (`output::success` and a raw `eprintln!`) and the terminal was to be unchanged, so the inconsistency is inherited on purpose rather than corrected inside a refactor whose acceptance criterion forbade it. Recorded so the next person to notice finds the decision.

Four small behaviours shifted. None is a message's text, glyph, colour or gating:

| Shift                                                                                          | Observable                                                                                          |
| ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| A failed prune clears its bar before reporting, where it left it spinning                      | Yes, on failure; strictly tidier                                                                    |
| Prune's `--verbose` subprocess echo prints before the bar clears, not after                    | `-v` only, and it makes prune match push, which always echoed under a live bar                      |
| Whitespace-only prune output no longer prints a blank line                                     | Yes; pinned by `prune_with_nothing_to_report_emits_no_line`                                         |
| The App's bar is created before its destination directory, so the mkdir-failure path shows one | Yes, on a failure that has not been seen; the factory has to be called before it can report through |

One behaviour outside the Backup Session changed as a consequence of the `chrome` gate: `sync music --dry-run --quiet` no longer prints `→ Would transfer …`. That line reached `progress.info` and so escaped a `--quiet` that was already suppressing the three `output::info` lines around it — the same inconsistency, one command over. Suppressed rather than preserved, because the renderer standing in for `output::info` has to behave like it.

### Not universal: the verbose subprocess mirror

One output path in the module is deliberately left outside the seam. `probe_repo`, `init_repo` and `forget_snapshots` still call `output::subprocess_output("restic", …)` and `output::clear_subprocess_lines(…)` — the `--verbose` echo of a subprocess's own stderr, dim-prefixed `restic | …` and erased with cursor-up escapes when the command succeeds. It is invisible to `MockProgress` and the fence does not catch it.

It is out for two reasons, not one, and both matter. It is not a _message the runner emits_: it is a mirror of a child process's stderr, shared by every subprocess in the crate (`ansible_runner`, `run_piped`, `stream_command_*`), gated on the global verbose flag whose teardown this decision explicitly does not attempt. And its erase step writes raw cursor-up sequences, which `ProgressBar::println` would interleave rather than pass through — routing it through `line` would corrupt it.

Named here rather than left implicit, because "every message is an event" stated flat is broader than the fence holds, and a claim wider than its guard is the failure ADR-0046 records at length. The rule as fenced is: **no `not(test)` under `src/`, and no `TerminalProgress::` outside `src/commands/`.** The event-stream half is a convention the two fences do not reach, and the remaining exception is this one.

## Why

The crate held exactly one compile-time seam, and it had quietly taken a module out of the test suite without taking it out of the coverage story.

`BackupSession::create` reached its Progress through `make_recipe_progress`, forked on `cfg(test)`: a live `TerminalProgress` in the shipped binary, a hidden one in the test binary. `MockProgress` exists and is used twenty times by the Recipe Executor's tests one module away — and could never observe a Session, because the Session never accepted one. So `create`'s own output was unasserted, and it had drifted into three paths in one function: Progress for the executor, `output::success` gated on `is_verbose()`, and a raw `eprintln!` for failures.

The cost compounded next door. `restic_push` and `restic_prune` each constructed a `TerminalProgress` outright, and between them had **zero tests**. Push in particular carries the branch that creates the repository when none exists — reachable only against a repository that genuinely does not exist, which is a thing that happens once, by hand, and never again on that host.

| Function       | Before                          | After                                               |
| -------------- | ------------------------------- | --------------------------------------------------- |
| `create`       | hidden bar under `cfg(test)`    | injected factory; 4 new tests over its event stream |
| `restic_push`  | own `TerminalProgress`, 0 tests | injected Progress + `drive_restic_push`; 6 tests    |
| `restic_prune` | own `TerminalProgress`, 0 tests | injected Progress + `drive_restic_prune`; 4 tests   |

The split is the one `sync music` already had: a driver owning ordering and output over closures standing for the subprocess calls, and wiring owning the subprocesses. It is what makes "does an absent repository get initialized before the push?" a test rather than an inspection.

25 tests added, 909 to 934.

### Why a factory rather than one Progress

A single `&mut dyn Progress` threaded through `create` is smaller and was rejected on terminal behaviour, not on principle: the Session's Progress is per-App, because a bar that says `Backing up navidrome` is the bar for navidrome. One shared Progress would have to be finished thirteen times or not at all, and either way the per-App bar is gone. The factory keeps lifetime and ownership where they already were — one Progress created, used and dropped per iteration — and adds exactly one thing: the caller decides what kind.

Its argument is the App name, which is what the initial message is built from. That the factory is _called_ once per App, in recipe order, is itself asserted, so a future change that hoisted the call out of the loop would fail rather than silently collapse the bars.
