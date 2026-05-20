# ADR-0008: WebDAV role removed in favor of Gokapi

## Status

Accepted, 2026-05-20.

## Decision

Delete the `webdav` ansible role and its supporting Caddy plugin (`caddy-webdav`). Link-share use cases move to the new `gokapi` Public App (issue #338).

## Why

The `webdav` role was deployed as a single-user Caddy WebDAV endpoint over `/var/www/webdav-files` with HTTP Basic auth. In practice the operator used it only as a "upload a file, send the URL to a friend" channel — not as a mounted network filesystem. Audit of the operator's workflow confirmed:

- No device mounts `dav.{domain}` as a filesystem (iOS Files, GNOME Files, rclone, Joplin, etc.).
- No script issues WebDAV-protocol verbs (`PROPFIND`, `MKCOL`, `PUT`-in-place).
- The actual workflow was strictly "drop file in `/var/www/webdav-files`, share the resulting URL."

For the strict "drop and share" workflow, Gokapi is strictly superior on every axis:

- Per-share expiry (download-count or time-based) replaces the previous "the file lives forever once uploaded" model.
- File Requests give friends an upload-only URL — a capability WebDAV could only offer by handing out the admin password.
- Optional per-share password is per-link, not a single shared secret.
- Memory footprint is comparable (~15-30 MB) and dependencies are simpler (no Caddy plugin recompile).

WebDAV's persistent-mounted-filesystem capability is preserved by Syncthing, which is already in the stack and was the actual canonical answer for "I want my files synced to a device."

## Considered alternatives

- **Keep both roles indefinitely.** Rejected: WebDAV had a known, narrow use case that Gokapi fully covers; carrying two roles for one workflow is operational debt. The `caddy-webdav` plugin also lengthens every Caddy rebuild by adding a `go get` + compile step for a module the operator no longer needs.
- **Deprecate WebDAV but keep the role for one release.** Rejected: there is no migration concern. The data store is `/var/www/webdav-files` and is preserved by restic's existing snapshots up to and including the snapshot prior to deletion. If the operator ever wants a file back, they `restic restore` it directly to disk — no role needed.
- **Replace WebDAV with a Tailnet-only file server.** Rejected: solves a different problem (intra-tailnet shared filesystem), which Syncthing already covers without an HTTP layer.

## Consequences

**Positive:**

- Caddy rebuilds no longer pull and compile `github.com/mholt/caddy-webdav` — `caddy_build_id` rotation will trigger one final rebuild on next deploy, then the plugin is gone.
- Key Registry loses `webdav_password` (secret) and `webdav_subdomain` — one fewer secret to rotate.
- The Public App list is unambiguous about who owns "share a file by URL": Gokapi.

**Negative:**

- Any operator who _did_ mount `dav.{domain}` as a filesystem loses that capability. Audit before this ADR was accepted confirmed no such mount exists for the current operator; future contributors with different workflows should consult Syncthing or run a one-off WebDAV server outside auberge.
- Existing restic snapshots containing `/var/www/webdav-files` continue to exist until pruned. They remain restorable via `restic restore`; no migration tooling is needed.

## References

- Issue #338 — Gokapi as a Public App; introduces the replacement.
- ADR-0001 — Declarative Backup Recipes. The principle that an App's whole identity is its role + meta makes removal a single coordinated delete, not a multi-step decommission.
