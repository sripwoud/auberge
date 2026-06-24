# Contributing

## Process

1. Fork repository
2. Create feature branch
3. Make changes with tests
4. Submit pull request

## Code Style

```bash
cargo fmt && cargo clippy  # Rust
ansible-lint                # Ansible
```

Use conventional commits:

```
feat(scope): description
fix(scope): description
```

## Testing

All PRs require:

- Tests for new code
- `cargo test` passing
- `ansible-lint` passing
- Updated documentation

## Development

```bash
./setup           # Install dependencies
cargo build       # Build
cargo test        # Test
```

See [Development Setup](development/setup.md) for details.
