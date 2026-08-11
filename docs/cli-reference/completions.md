# auberge completions

Print a static shell completion script to stdout, per [ADR-0018](https://github.com/sripwoud/auberge/blob/master/meta/adr/0018-static-shell-completion-via-completions-subcommand.md). No alias.

```bash
auberge completions <SHELL>
```

## Arguments

| Argument  | Description                                      |
| --------- | ------------------------------------------------ |
| `<SHELL>` | `bash`, `zsh`, `fish`, `elvish`, or `powershell` |

## What completes

Everything the binary knows at compile time: subcommands, their aliases (`dp`, `hs`, `ss`, …), flags, and enum values such as `--output {human,json}`.

Runtime values do not complete — `auberge deploy <TAB>` will not suggest App names, `-H <TAB>` will not suggest Hosts. Omit the value instead: the [interactive picker](cli-reference/select/host.md) prompts with fuzzy search.

## Install

Recommended — regenerate on every shell start, so the script can never drift from the installed binary:

```bash
# ~/.bashrc — after auberge is on PATH (e.g. after mise activation)
source <(auberge completions bash)
```

Alternative — one-time file drop, lazy-loaded on first TAB (zero startup cost):

```bash
auberge completions bash > ~/.local/share/bash-completion/completions/auberge
```

!> The file drop goes stale on every upgrade until re-run — a new subcommand ships and the script doesn't know it. Prefer the `source` line unless shell startup time matters more.

See [Installation](getting-started/installation.md) for setup context.
