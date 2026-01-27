# Testing

## Rust

```bash
cargo test
cargo clippy
```

## Ansible

```bash
ansible-playbook playbooks/auberge.yml --syntax-check
ansible-lint
auberge ansible check --tags myapp  # Dry run
```

## Manual Testing

```bash
cargo build
./target/debug/auberge host list
```
