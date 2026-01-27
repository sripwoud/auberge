# Installation

## Prerequisites

- Rust and Cargo (latest stable)
- A VPS with root/sudo access
- SSH connectivity to your VPS

## Install Rust

If you don't have Rust installed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the prompts and restart your shell.

## Install Auberge

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

Not officially supported. Use WSL2 with Linux instructions.

## Next Steps

- [Quick Start](quick-start.md) - Get started immediately
- [First Deployment](first-deployment.md) - Detailed deployment guide
