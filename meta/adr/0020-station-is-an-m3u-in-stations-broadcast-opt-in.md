# ADR-0020: A Station is an m3u file in `Stations/`; broadcast is opt-in by directory

## Status

Accepted, 2026-08-12. Recorded before implementation — the build is tracked in a separate issue. This ADR fixes the design so the implementation has nothing left to decide.

The rsync flags and exclude patterns quoted below were current when this was written; both have since changed (#487, #492). The property this ADR depends on — that the sync carries `.m3u` without naming it — is unchanged and now recorded as a decision in ADR-0022.

## Decision

Auberge gains a **Radio**: a Public App running Icecast2 and Liquidsoap, both from apt, serving one or more **Stations** of the user's own music.

- A **Station** is exactly one `.m3u` file under `<MusicFolder>/Stations/`, encoded by Liquidsoap and served at one Icecast mountpoint. Adding a file adds a station; the filename names it.
- **Broadcast is opt-in by directory.** `<MusicFolder>/Playlists/` holds playlists that are Navidrome-only. `<MusicFolder>/Stations/` holds playlists that are _also_ broadcast. Liquidsoap globs `Stations/*.m3u` and nothing else. Every Station is a playlist; a playlist is a Station only by its location.
- **Curation happens in beets**, not in a web UI and not in `config.toml`. A station is the output of a beets query (`beet ls -f '$path' <query> > Stations/focus.m3u`), and reaches the Host through the existing `auberge sync music`, whose rsync carries every non-excluded file — `MUSIC_RSYNC_FLAGS` is `-rltzvP --omit-dir-times` with only `--exclude=.DS_Store` and `--exclude=*.tmp` (`src/commands/sync.rs:10,97-98`).
- **One `.m3u` file is simultaneously a Navidrome playlist and a Station.** Navidrome's `PlaylistsPath` imports the same files it broadcasts from. There is no export step, no timer, and no second copy that can drift.
- **Listener auth is one shared credential in Caddy**, covering every mount on the domain. It follows `colporteur` exactly: the plaintext lives in the **Key Registry**, and the role hashes it at deploy with `caddy hash-password` under `no_log: true`, then renders the hash into the Caddyfile (`ansible/roles/colporteur/tasks/main.yml:214-227`, `templates/Caddyfile.j2:2-4`).
- **No App Version** — Icecast2 and Liquidsoap come from apt (see "The apt exception" below).
- **No Backup Recipe.** Icecast and Liquidsoap hold no state: configs are role-rendered and reproducible by deploy, and the m3u files' canonical copy is the operator's local `~/Music`, from which they are synced. 11 of 22 Playbook Metas already omit a `backup:` section, and `CONTEXT.md:36` defines an App as having a Backup Recipe _iff_ the section is present.

## Why

### The audience boundary is the entire point of the feature

The Radio exists because on-demand sharing does not give a shared timeline, and it is gated because an ungated one would not be a private circle. Under EU copyright the restricted act is _communication to the public_ (InfoSoc Art. 3), and the test the CJEU applies — an indeterminate audience, counted cumulatively across successive listeners (_SGAE v Rafael Hoteles_, C-306/05) — is not satisfied by an unlisted URL. Obscurity establishes no boundary: no enumerable membership, no revocation, nothing to point at. What counts is a technical restriction measure (_Svensson_, C-466/12; _VG Bild-Kunst_, C-392/19), which is what the Caddy credential is.

That makes publishing the one genuinely irreversible act in this design, and it decides the layout. A single directory in which every `.m3u` becomes a mount would make _dropping a file_ an act of publication — privacy by remembering, publication by default. The directory split inverts that: the operator cannot broadcast without moving a file into `Stations/`.

### Why a directory and not a naming convention

`radio-*.m3u` produces the same behaviour and is defeated by one typo, by one agent that writes `focus.m3u`, by one `beet ls` redirect with the prefix forgotten. Nothing catches it and the failure mode is silent publication. A path glob has no such failure: a file is in `Stations/` or it is not.

### Why files are the source of truth, not Navidrome's database

`CONTEXT.md` already treats Navidrome as a read-only index over the filesystem — it never writes tags, which is precisely why beets can own the library. Curating stations in Navidrome's web UI and exporting them would invert that for playlists alone: the database becomes authoritative for one class of object and a synchronisation process becomes load-bearing. Navidrome's export is [one playlist per invocation](https://github.com/navidrome/navidrome/issues/1914) with no bulk-to-disk mode, so the sync would be a scripted loop on a timer, and its round trip has a [known defect — exported playlists cannot be re-imported](https://github.com/navidrome/navidrome/issues/4530).

Keeping the m3u authoritative removes the timer, the script, and the drift. It also makes a station a beets query, which is the one part of this the operator already automates and already has tooling for.

### The apt exception

`CONTEXT.md:67` records that "Caddy has _no_ App Version (Caddy itself comes from apt), which is why it needs no meta file despite carrying two pins." The Radio is the second instance and the first that still needs a meta file — for `subdomain:`, without a `version:` block.

This surfaced a contradiction: `CONTEXT.md:165` asserted that "every App declares one" App Version, which Caddy already falsified. ADR-0017 chose Pinned Version Resolution against Floating and Latest-at-Deploy; it did not claim jurisdiction over packages whose version the distribution chooses. Corrected in `CONTEXT.md` alongside this ADR: an App installed from apt has no App Version, because apt source priority — not the repo — decides which bytes land.

## Considered alternatives

- **One directory; every playlist is a station.** Rejected on the reasoning above: it makes publication the default for the only irreversible act in the design.

- **A `radio-*.m3u` naming convention.** Rejected: unenforceable, and it fails silently in the publishing direction.

- **Station definitions in `config.toml`, rendered into the Liquidsoap script.** Precedented (`colporteur_config_src` reads an operator config file) and the _most_ deliberate option — a station could not exist without a reviewed repo change. Rejected on granularity: station identity would split across two places (the list in `config.toml`, the content in the m3u), and adding or reordering a station would require a deploy. Stations are curated iteratively; a redeploy per edit is the wrong unit.

- **Curate in Navidrome's web UI, export to m3u on a timer.** Rejected above. It is the better _curation_ experience — searchable, drag-and-drop, usable from a phone while listening — and it was rejected only after establishing that curation here is a deliberate at-the-desk activity, never a from-the-couch one. If that ever changes, this is the alternative to revisit, and the cost of revisiting is a scheduled exporter plus accepting two sources of truth.

- **Endless shuffle of the whole library, no stations.** ~15 lines of Liquidsoap, nothing to maintain, new music enters rotation automatically. Rejected because it is not a station: shuffle-everything is available from any Subsonic client and expresses no selection. Recorded because it remains the correct answer for anyone who wants "music is always on" rather than "listen to what I chose".

- **Icecast per-mount `htpasswd` instead of Caddy `basic_auth`.** Rejected: `colporteur` is an exact precedent for the Caddy path including the Key-Registry-to-hash flow, and Icecast's own auth would be a second auth system in the repo for one gain — a different password per mount — that nothing asks for. Revisit if per-station audiences are ever wanted; Icecast keeps per-mount listener stats and limits that Caddy cannot see.

- **Tailnet-only, no listener auth.** The strongest possible circle and it needs no credential at all. Rejected as unusable: Tailscale cannot be installed on the listening hardware in question, and requiring a VPN enrolment to hear an album defeats the purpose.

- **Public, unauthenticated.** Rejected on the audience reasoning above.

- **AzuraCast.** A complete station manager with a web UI, which would have answered the curation question outright. Rejected: it is Docker-only, and a container is a per-app exception granted only to an App with no native alternative (`meta/adr.md` §"Native systemd by default", ADR-0025) — the m3u design below is that alternative. It would also drag in MariaDB, Redis and a second reverse proxy.

## Consequences

**Positive:**

- Publication requires a deliberate filesystem act. There is no path by which curating a playlist broadcasts it.
- One artifact per station. No exporter, no timer, no drift between what Navidrome shows and what the Radio plays.
- Curation is scriptable and agent-maintainable — a station is a beets query, and the existing music-metadata tooling applies unchanged.
- Nothing new to back up, and nothing new to version. The App is reproducible from the repo plus apt.
- Station content is editable without a deploy. Liquidsoap can watch its playlist files, so editing an m3u changes the station live.

**Negative:**

- Curation is desk-bound. Building a station from a phone while listening is not possible, by construction.
- Each Station costs a Liquidsoap encoder running continuously whether or not anyone is listening, because Icecast is push-based. Egress is only incurred per connected listener — the Liquidsoap-to-Icecast hop is localhost — but CPU is not. This bounds the sensible number of stations to a handful on a small VPS; it is not a design that scales to dozens.
- One shared credential means one revocation. Removing one friend's access changes the password for everyone.
- Station names are filenames, so renaming a station breaks any URL a listener saved.
- Two directories to keep straight, which is pure ceremony for anyone who only ever wants stations and no private playlists.

**Unverified — settle during implementation, do not assume:**

- Whether Navidrome's `PlaylistsPath` recurses or accepts multiple paths. If it does neither, `Stations/` nests _inside_ `Playlists/`; the reasoning and the Liquidsoap glob are unaffected.
- Caddy's response buffering against a `Content-Length`-less Icecast stream. `flush_interval -1` on the `reverse_proxy` is the expected requirement.
- Icecast's `<public>` directive must be off — it registers the station in the Xiph YP directory — and the default `/status.xsl` mount listing must not be reachable through Caddy. An Icecast install is not private by default, and both are audience leaks that the Caddy credential does not cover.
- Navidrome's m3u import has [a reported defect on first library import](https://github.com/navidrome/navidrome/issues/3109). It affects Navidrome's view only; Liquidsoap reads the files directly.

## References

- ADR-0003 — the Public App / Tailnet-only App split this chooses between. The Radio is a Public App with app-layer auth, which `colporteur` already establishes as a third position rather than a new one.
- ADR-0016 — the bare-metal-from-a-package-manager precedent, and the origin of the two-regimes observation ADR-0017 resolved.
- ADR-0017 — App Version versus Tool Version. The Radio has neither: apt chooses the bytes. Its `CONTEXT.md` overreach ("every App declares one") is corrected here.
- `meta/adr.md` §"Native systemd by default" and ADR-0025 — why AzuraCast is not an option: it clears the upstream-support test but fails the one asking whether the App is required.
- `ansible/roles/colporteur/` — the auth pattern copied verbatim: `keys.yml` plaintext, `caddy hash-password` at deploy, hash rendered into the Caddyfile.
- `src/commands/sync.rs:10,97-98` — the rsync flags that make m3u transport free.
- `CONTEXT.md` — **Radio**, **Station**, **Public App**, **Key Registry**, **Backup Recipe**.
- Issue #179 — proposes migrating auberge from Ansible to NixOS. A new Ansible role carries shelf-life risk against it; the decisions in this ADR are substrate-independent and survive the migration, but the role would be rewritten.
