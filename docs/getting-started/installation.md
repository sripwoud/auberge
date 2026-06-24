# Installation

| Platform                                       | Method                                                                                                     |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Linux (x86_64, aarch64), macOS (Apple Silicon) | Download from [latest release](https://github.com/sripwoud/auberge/releases/latest), extract onto `$PATH`. |
| Any platform with [Rust](https://rustup.rs/)   | `cargo install auberge` (installs to `~/.cargo/bin/`).                                                     |
| Windows                                        | Not supported — Ansible requires a Unix control node. Use WSL2.                                            |

```bash
auberge --version            # verify
cargo install auberge --force # update
cargo uninstall auberge       # remove
```

No platform-specific dependencies on Linux or macOS.

Continue: [Quick Start](getting-started/quick-start.md) · [First Deployment](getting-started/first-deployment.md)
