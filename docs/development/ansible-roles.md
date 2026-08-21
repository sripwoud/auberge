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
auberge deploy myapp --check  # Dry run
auberge deploy myapp          # Deploy
```

## Versions

Two kinds of version, two locations:

| Kind         | Lives in                           | Managed as                                   |
| ------------ | ---------------------------------- | -------------------------------------------- |
| App Version  | `ansible/playbooks/<app>.meta.yml` | typed `version:` block                       |
| Tool Version | role `defaults/main.yml`           | `<role>_version:` + `# renovate:` annotation |

**App Version** — the app itself (e.g. Baikal). Declared once in Playbook Meta:

```yaml
version:
  value: "0.12.1"
  datasource: github-releases
  depName: "sabre-io/Baikal"
```

`datasource` is `npm` or `github-releases`; `versioning` / `extractVersion` are optional. Auberge injects this as an extra var at deploy — role tasks reference `{{ <role>_version }}` unchanged. Roles must NOT define `<role>_version` in `defaults/main.yml`.

**Tool Version** — a build or runtime input the role needs (`uv`, `lego`, a Caddy plugin), not the app itself. Stays in `defaults/main.yml`, preceded by a renovate annotation:

```yaml
# renovate: datasource=github-releases depName=astral-sh/uv
hermes_uv_version: "0.7.0"
```

Renovate reads both locations via regex custom managers (`renovate.json`) and opens `build(<app>): bump to X` PRs. Roles that co-pin a sha256 checksum keep it as a literal; `renovate-checksums.yml` recomputes the digest on Renovate PRs and pushes the fix to the same branch — a bump whose digest can't be computed stays red and doesn't merge.

CI tests assert every app playbook declares an App Version and every remaining `_version:` in role defaults carries a renovate annotation.

See [ADR-0017](https://github.com/sripwoud/auberge/blob/master/meta/adr/0017-app-versions-declared-in-playbook-meta.md) for the rationale.

## Best Practices

- Use `ansible.builtin.*` modules (idempotent)
- Avoid raw commands unless necessary
- Use handlers for service restarts; restart inline when the app cannot serve until the unit reloads, and assert the result in the same play
- Name handlers per role (`Restart baikal php-fpm`): handler names share one namespace per play, so a duplicate is silently shadowed by the last role loaded
- Template config files with variables
- Tag roles appropriately
- Include `dns_record` role for web-accessible apps with a public subdomain
- Declare the App Version in `<app>.meta.yml`, never in `defaults/main.yml`
