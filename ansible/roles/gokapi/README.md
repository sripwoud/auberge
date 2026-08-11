# Gokapi Ansible Role

Deploys [Gokapi](https://github.com/Forceu/Gokapi) — a Go single-binary, self-hosted Firefox Send alternative — as a systemd service behind Caddy.

## Features

- Link-based file sharing with expiry by download count or time
- File Requests: per-URL upload links visible only to the creator
- Optional per-share password
- SQLite backend (default) or S3
- Optional OIDC integration

## Variables

| Variable                        | Default                                 | Description                                                |
| ------------------------------- | --------------------------------------- | ---------------------------------------------------------- |
| `gokapi_install_path`           | `/opt/gokapi`                           | Binary install directory                                   |
| `gokapi_data_dir`               | `/var/lib/gokapi`                       | Persistent data + DB directory                             |
| `gokapi_config_dir`             | `/var/lib/gokapi/config`                | Config directory (`config.json` lives here)                |
| `gokapi_sys_user`               | `gokapi`                                | System user                                                |
| `gokapi_sys_group`              | `gokapi`                                | System group                                               |
| `gokapi_port`                   | `53842`                                 | Local port for Caddy reverse proxy                         |
| `gokapi_domain`                 | `{{ gokapi_subdomain }}.{{ domain }}`   | Public hostname                                            |
| `gokapi_version`                | `2.2.4`                                 | Pinned upstream release                                    |
| `gokapi_bootstrap_marker`       | `{{ gokapi_data_dir }}/.bootstrap_done` | Sentinel file gating the one-shot superadmin bootstrap     |
| `gokapi_public_name`            | `Gokapi`                                | `<title>`, `<h1>` and Open Graph name (not the hostname)   |
| `gokapi_custom_dir`             | `{{ gokapi_data_dir }}/custom`          | On the host; served at `/custom`                           |
| `gokapi_custom_assets_src`      | `~/.config/auberge/gokapi/custom`       | On the deploying machine; optional branding, may be absent |
| `gokapi_custom_assets_revision` | `2`                                     | Cache-buster for every asset under `custom/`               |

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

**Branding is opt-in and ships nothing.** The role deliberately carries no
stylesheet and no icon, so a default deploy looks like stock Gokapi. Both are
operator-supplied, read from `gokapi_custom_assets_src` on the machine you deploy
from. Leave that directory absent and nothing is deployed.

Gokapi serves anything in `gokapi_custom_dir` at `/custom` and reads four
well-known filenames from it at startup: `custom.css`, `favicon.png`, `admin.js`
and `public.js`. Whatever is in `gokapi_custom_assets_src` is copied there
verbatim, so any subset of those works.

To brand an instance:

```bash
mkdir -p ~/.config/auberge/gokapi/custom
# optional: a stylesheet, loaded after Bootstrap and Gokapi's own CSS
$EDITOR ~/.config/auberge/gokapi/custom/custom.css
# optional: a 512x512 PNG, see Favicons below
cp my-icon.png ~/.config/auberge/gokapi/custom/favicon.png
```

then set the display name in `~/.config/auberge/config.toml`:

```toml
gokapi_public_name = "Files"
```

`gokapi_public_name` sets only the display name (`<title>`, the `<h1>`, and the
Open Graph tags). The hostname comes from `gokapi_subdomain`, so the two can
differ — an instance can be reached at a lowercase subdomain while presenting
with a capital.

Because `config.json` is written once with `force: false`, a rename cannot land
by re-templating. A `replace` task migrates `PublicName` instead, matching
Gokapi's own default (`Gokapi`) **plus every value this role has ever defaulted
to**. Anything else is treated as an operator edit made through the admin UI and
left alone. If this role's default ever changes again, add the superseded value
to that task's regexp or already-deployed hosts will silently keep the old name.

### Writing a stylesheet

Worth knowing before you start: Gokapi's shipped bundle
(`css/min/gokapi.min.*.css`) sets a photographic `background.jpg` on `body`, a
blurred `box-shadow` vignette on `body::after`, and `filter: brightness(80%)` on
`a:hover`. Any theme has to neutralise all three. Also, `body.bg-dark` from
Bootstrap outranks a plain `body` rule even when both are `!important`, because
specificity decides once importance ties.

### Favicons

Gokapi's favicon `<link>`s are hardcoded in `html_header.tmpl` and point at files
compiled into the binary with `//go:embed`, so a PNG named after any of those
links does nothing. Instead Gokapi reads one file, `custom/favicon.png`, and
derives `favicon.ico` plus the 16, 32, 180, 192 and 512px variants from it at
startup (`internal/webserver/favicon/Favicon.go`). No JavaScript is involved.

That file must be **exactly 512x512** or Gokapi prints a message and calls
`os.Exit(1)` — it does not fall back to its default icon, so a wrong-sized asset
stops the service from starting. The role reads the dimensions from the PNG IHDR
chunk on the deploying machine and fails the run before touching the host, so the
mistake never reaches a restart.

Two things worth doing to the source image:

- **Give it an opaque background** rather than transparency, unless the artwork
  reads on both light and dark. Browsers draw the tab icon on their own chrome,
  which follows the user's theme.
- **Build it as a small pixel grid scaled up by an integer factor** if it is pixel
  art. Gokapi derives every size with `draw.NearestNeighbor`, so a 32x32 grid
  scaled 16x to 512 gives exact subsamples at the 16px and 32px a browser tab
  requests (`512/32` and `512/16` are both integers) instead of aliasing.

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
