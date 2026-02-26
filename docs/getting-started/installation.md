# Installation

## Prerequisites

- A VPS with root/sudo access
- SSH connectivity to your VPS

## Pre-compiled binaries

Download a binary for your platform from the [latest release](https://github.com/sripwoud/auberge/releases/latest). Binaries are available for Linux (x86_64, aarch64) and macOS (Apple Silicon).

Extract it somewhere on your `PATH`, e.g. `~/.local/bin`.

## Install from crates.io

Requires [Rust](https://rustup.rs/).

```bash
cargo install auberge
```

This compiles and installs the `auberge` binary to `~/.cargo/bin/`.

## Verify Installation

```bash
auberge --version
```

## Update Auberge

```bash
cargo install auberge --force
```

## Uninstall

```bash
cargo uninstall auberge
```

## Platform-Specific Notes

### Linux

No additional dependencies required.

### macOS

No additional dependencies required.

### Windows

Not supported. Ansible requires a Unix-based control node (Linux or macOS) — it cannot run from Windows natively. Use WSL2 with the Linux instructions above.

## Next Steps

- [Quick Start](quick-start.md) - Get started immediately
- [First Deployment](first-deployment.md) - Detailed deployment guide
