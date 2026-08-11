# Gokapi Ansible Role

Deploys [Gokapi](https://github.com/Forceu/Gokapi) — a Go single-binary, self-hosted Firefox Send alternative — as a systemd service behind Caddy.

## Features

- Link-based file sharing with expiry by download count or time
- File Requests: per-URL upload links visible only to the creator
- Optional per-share password
- SQLite backend (default) or S3
- Optional OIDC integration

## Variables

| Variable                        | Default                                 | Description                                              |
| ------------------------------- | --------------------------------------- | -------------------------------------------------------- |
| `gokapi_install_path`           | `/opt/gokapi`                           | Binary install directory                                 |
| `gokapi_data_dir`               | `/var/lib/gokapi`                       | Persistent data + DB directory                           |
| `gokapi_config_dir`             | `/var/lib/gokapi/config`                | Config directory (`config.json` lives here)              |
| `gokapi_sys_user`               | `gokapi`                                | System user                                              |
| `gokapi_sys_group`              | `gokapi`                                | System group                                             |
| `gokapi_port`                   | `53842`                                 | Local port for Caddy reverse proxy                       |
| `gokapi_domain`                 | `{{ gokapi_subdomain }}.{{ domain }}`   | Public hostname                                          |
| `gokapi_version`                | `2.2.4`                                 | Pinned upstream release                                  |
| `gokapi_bootstrap_marker`       | `{{ gokapi_data_dir }}/.bootstrap_done` | Sentinel file gating the one-shot superadmin bootstrap   |
| `gokapi_public_name`            | `Partage`                               | `<title>`, `<h1>` and Open Graph name (not the hostname) |
| `gokapi_custom_dir`             | `{{ gokapi_data_dir }}/custom`          | Served at `/custom`; holds the theme, icons and manifest |
| `gokapi_custom_assets_revision` | `2`                                     | Cache-buster for every asset under `custom/`             |

`gokapi_admin_user` and `gokapi_admin_password` are required keys from the operator's `config.toml` (declared in the Playbook Meta).

## Headless first-deploy

The role bootstraps Gokapi without the interactive setup wizard. See ADR-0009 for the rationale and security analysis.

1. `config.json` is templated at `{{ gokapi_config_dir }}/config.json` with `force: false`. This means it is written exactly once, then never overwritten — Gokapi's admin UI persists its own changes to the same file via `configuration.save()`, and those operator edits must survive subsequent deploys.
2. Before the systemd service starts, `gokapi --deployment-password <pw>` runs once as the `gokapi` user. Gokapi's `EditSuperAdmin` creates the superadmin in its database (Username from the templated `config.json`, password hashed by Gokapi using its own KDF), then exits 0.
3. The bootstrap marker (`{{ gokapi_bootstrap_marker }}`) is touched. Subsequent deploys see the marker and skip the one-shot.
4. The systemd service is enabled and started. `RunIfFirstStart` sees `config.json` exists and never opens the setup webserver; `checkIfUserExists` finds the superadmin and passes.
5. Caddy site and Cloudflare A record deploy unconditionally in the same `auberge deploy` invocation.

## Rotating the admin password

Auberge does not propagate password changes automatically (the marker file blocks the one-shot from re-running). To rotate, delete the marker (path is `gokapi_bootstrap_marker`, default `{{ gokapi_data_dir }}/.bootstrap_done`) and redeploy:

```bash
ssh <host> sudo rm /var/lib/gokapi/.bootstrap_done   # default path; adjust if you override gokapi_bootstrap_marker or gokapi_data_dir
auberge deploy gokapi
```

The next deploy sees the marker missing, re-runs `--deployment-password` with the new value from `config.toml`. `EditSuperAdmin` updates the existing superadmin's password and exits 0.

## Branding

Gokapi serves anything in `gokapi_custom_dir` at `/custom`, and reads four
well-known filenames from it at startup: `custom.css`, `favicon.png`, `admin.js`
and `public.js` (only the first two are used here).

The theme borrows Monkey Island 1's _technique_ — a strictly limited palette with
ordered 2px dithering instead of gradients, hard pixel edges, no blur — but not
its palette. The 16 colours are the operator's own site identity (`#120548`
background, `#908DFF` text, `#D000E8` accent, from `sripwoud.github.io`) with a
`#5417B8` violet field, plus a handful of saturated accents for state. See the
token block at the top of `files/custom/custom.css`.

Dithering is what carries the look, not the specific hexes: EGA had no
intermediate colours, so its artists checkerboarded two palette entries and let
the eye blend them. Smooth CSS gradients would read as generic dark mode.

`gokapi_public_name` sets only the display name (`<title>`, the `<h1>`, and the
Open Graph tags). The hostname comes from `gokapi_subdomain`, so the two can
differ — the site is reached at a lowercase subdomain while presenting as
`Partage`.

Because `config.json` is written once with `force: false`, a rename cannot land
by re-templating. A `replace` task migrates `PublicName` instead, matching
Gokapi's own default (`Gokapi`) **plus every value this role has previously
shipped**. Anything else is treated as an operator edit made through the admin
UI and left alone. When changing `gokapi_public_name`, add the superseded value
to that task's regexp or already-deployed hosts will silently keep the old name.

### Favicons

Gokapi's favicon `<link>`s are hardcoded in `html_header.tmpl` and point at files
compiled into the binary with `//go:embed`, so a PNG named after any of those
links does nothing. Instead Gokapi reads one file, `custom/favicon.png`, and
derives `favicon.ico` plus the 16, 32, 180, 192 and 512px variants from it at
startup (`internal/webserver/favicon/Favicon.go`). No JavaScript is involved.

That file must be **exactly 512x512** or Gokapi prints a message and calls
`os.Exit(1)` — it does not fall back to its default icon, so a wrong-sized asset
stops the service from starting. `test_gokapi_custom_favicon_is_512_square`
asserts the dimensions from the PNG IHDR chunk so CI catches it first.

`files/custom/favicon.png` is the operator's avatar remapped onto the theme
palette and flattened onto the `#5417B8` field. Flattened, not transparent: the
avatar is mostly black, which would disappear against a dark browser tab bar.

It is built as a 32x32 pixel grid scaled up 16x. That is deliberate — Gokapi
derives every size with `draw.NearestNeighbor`, and `512 / 32 = 16` and
`512 / 16 = 32` are both integers, so the 16px and 32px icons a browser tab
actually requests come out as exact subsamples rather than aliased mush.

### Changing an asset

Bump `gokapi_custom_assets_revision`, which renders `custom/version.txt`. Gokapi
reads that at startup and embeds it in the stylesheet filename it serves
(`/custom/custom.v<N>.css`), so a bump changes the URL and every browser refetches.
This matters because Gokapi sets a 2-day `Cache-Control` on everything under
`/custom`.

Note the asymmetry: `serveCustomFile` reads from disk on every request, but the
version is read **once at startup**. Editing `custom.css` in place therefore
needs no restart — a hard reload picks it up. Only changing `version.txt`
requires one, because the filename is baked into the templates at boot.

## Management

```bash
sudo systemctl status gokapi
sudo journalctl -u gokapi -f
sudo systemctl restart gokapi
```

## Backup

`/var/lib/gokapi` contains the SQLite DB and uploaded files. The Playbook Meta's Backup Recipe captures it.

## Dependencies

- Caddy reverse proxy (substrate)
- Cloudflare DNS API token (`cloudflare_dns_api_token`)
- UFW must allow 443/tcp on the public interface (default in `ufw` role)
