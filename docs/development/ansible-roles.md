# Ansible Roles

## Role Structure

```
roles/myapp/
├── tasks/main.yml
├── templates/
├── handlers/main.yml
└── defaults/main.yml
```

## Creating a Role

Key files:

**tasks/main.yml**

```yaml
- name: Install myapp
  ansible.builtin.get_url:
    url: https://github.com/author/myapp/releases/latest/myapp
    dest: /usr/local/bin/myapp
    mode: "0755"

- name: Deploy systemd service
  ansible.builtin.template:
    src: myapp.service.j2
    dest: /etc/systemd/system/myapp.service
  notify: Restart myapp

- name: Enable service
  ansible.builtin.systemd_service:
    name: myapp
    enabled: true
    state: started
```

**handlers/main.yml**

```yaml
- name: Restart myapp
  ansible.builtin.systemd_service:
    name: myapp
    state: restarted
```

## Adding to Playbook

Edit `ansible/playbooks/apps.yml`:

```yaml
- role: myapp
  tags: [apps, myapp]
```

## Testing

```bash
auberge ansible check --tags myapp  # Dry run
auberge ansible run --tags myapp    # Deploy
```

## Best Practices

- Use `ansible.builtin.*` modules (idempotent)
- Avoid raw commands unless necessary
- Use handlers for service restarts
- Template config files with variables
- Tag roles appropriately
