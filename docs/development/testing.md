# Testing

## Rust

```bash
cargo test
cargo clippy
```

## Ansible

```bash
ansible-playbook playbooks/apps.yml --syntax-check
ansible-lint
auberge deploy myapp --check  # Dry run
```

## Manual Testing

```bash
cargo build
./target/debug/auberge host list
```
