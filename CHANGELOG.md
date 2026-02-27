# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.14](https://github.com/sripwoud/auberge/compare/v0.3.13...v0.3.14) - 2026-02-27

### Added

- *(colporteur)* add ansible role for newsletter-to-feed service ([#86](https://github.com/sripwoud/auberge/pull/86))

## [0.3.13](https://github.com/sripwoud/auberge/compare/v0.3.12...v0.3.13) - 2026-02-27

### Added

- *(ci)* add sha256 checksums to release binaries

### Fixed

- *(ci)* drop windows target (ansible requires unix control node)
- *(ci)* drop x86_64-pc-windows-msvc target

### Other

- update platform support after dropping windows target

## [0.3.12](https://github.com/sripwoud/auberge/compare/v0.3.11...v0.3.12) - 2026-02-26

### Fixed

- *(ci)* make vendored openssl conditional on non-windows targets

### Other

- update README.md ([#84](https://github.com/sripwoud/auberge/pull/84))

## [0.3.11](https://github.com/sripwoud/auberge/compare/v0.3.10...v0.3.11) - 2026-02-26

### Fixed

- *(ci)* drop x86_64-apple-darwin target (macos intel runners deprecated)

## [0.3.10](https://github.com/sripwoud/auberge/compare/v0.3.9...v0.3.10) - 2026-02-26

### Fixed

- *(ci)* replace deprecated macos-13 runner with macos-15-large

## [0.3.9](https://github.com/sripwoud/auberge/compare/v0.3.8...v0.3.9) - 2026-02-26

### Fixed

- *(ci)* use correct release-plz output field for tag extraction

## [0.3.8](https://github.com/sripwoud/auberge/compare/v0.3.7...v0.3.8) - 2026-02-26

### Fixed

- *(ci)* vendor openssl for aarch64-linux cross-compilation

## [0.3.7](https://github.com/sripwoud/auberge/compare/v0.3.6...v0.3.7) - 2026-02-26

### Fixed

- *(ci)* pass release tag to binary upload action

## [0.3.6](https://github.com/sripwoud/auberge/compare/v0.3.5...v0.3.6) - 2026-02-26

### Fixed

- *(ci)* add cross-toolchain setup for aarch64-linux builds

### Other

- release v0.3.5 ([#76](https://github.com/sripwoud/auberge/pull/76))
- add pre-compiled binary install option ([#75](https://github.com/sripwoud/auberge/pull/75))
- add workflow_dispatch for manual binary builds
- add multi-platform binary releases ([#74](https://github.com/sripwoud/auberge/pull/74))
- add crates.io version badge to README

## [0.3.5](https://github.com/sripwoud/auberge/compare/v0.3.4...v0.3.5) - 2026-02-26

### Fixed

- *(ci)* add cross-toolchain setup for aarch64-linux builds

### Other

- add pre-compiled binary install option ([#75](https://github.com/sripwoud/auberge/pull/75))
- add workflow_dispatch for manual binary builds
- add multi-platform binary releases ([#74](https://github.com/sripwoud/auberge/pull/74))
- add crates.io version badge to README

## [0.3.4](https://github.com/sripwoud/auberge/compare/v0.3.3...v0.3.4) - 2026-02-16

### Added

- *(calendar)* add automatic birthday calendar sync ([#69](https://github.com/sripwoud/auberge/pull/69))

## [0.3.3](https://github.com/sripwoud/auberge/compare/v0.3.2...v0.3.3) - 2026-02-10

### Fixed

- *(freshrss)* enable automatic feed updates via systemd timer ([#66](https://github.com/sripwoud/auberge/pull/66))

## [0.3.2](https://github.com/sripwoud/auberge/compare/v0.3.1...v0.3.2) - 2026-02-09

### Added

- *(ansible)* add syncthing role for file synchronization ([#64](https://github.com/sripwoud/auberge/pull/64))

## [0.3.1](https://github.com/sripwoud/auberge/compare/v0.3.0...v0.3.1) - 2026-02-06

### Added

- *(ansible)* add openclaw deployment role ([#54](https://github.com/sripwoud/auberge/pull/54))

### Fixed

- deploy OpenClaw with dynamic bootstrap user support ([#63](https://github.com/sripwoud/auberge/pull/63))

### Other

- *(deps)* bump time from 0.3.45 to 0.3.47 in the cargo group across 1 directory ([#62](https://github.com/sripwoud/auberge/pull/62))
- *(deps)* bump bytes from 1.11.0 to 1.11.1 in the cargo group across 1 directory ([#53](https://github.com/sripwoud/auberge/pull/53))
- fix release_commits regex

## [0.3.0](https://github.com/sripwoud/auberge/compare/v0.2.0...v0.3.0) - 2026-02-03

### Added

- [**breaking**] replace Radicale with Baikal for CalDAV/CardDAV ([#50](https://github.com/sripwoud/auberge/pull/50))

### Other

- restructure documentation with `docsify` ([#48](https://github.com/sripwoud/auberge/pull/48))
- Create CNAME
- streamline README and link to documentation site

### Added

- **(breaking)** Replace Radicale with Baikal as CalDAV/CardDAV backend
  - New `baikal` Ansible role (PHP-FPM, Caddy, SQLite)
  - Playbook `remove-radicale.yml` for migration with optional backup
  - CLI backup/restore and DNS use `baikal` app; env: `BAIKAL_ADMIN_PASSWORD`, `BAIKAL_SUBDOMAIN`

## [0.2.0](https://github.com/sripwoud/auberge/compare/v0.1.4...v0.2.0) - 2026-01-26

### Added

- [**breaking**] reorganize backup structure to timestamp-first ([#45](https://github.com/sripwoud/auberge/pull/45))

### Other

- harden workflow permissions ([#46](https://github.com/sripwoud/auberge/pull/46))

## [0.1.4](https://github.com/sripwoud/auberge/compare/v0.1.3...v0.1.4) - 2026-01-26

### Added

- *(yourls)* automated deployment with database initialization ([#44](https://github.com/sripwoud/auberge/pull/44))
- improve CLI output ([#42](https://github.com/sripwoud/auberge/pull/42))

### Other

- Add --force/-f flag to skip confirmation prompts for CI/CD automation ([#39](https://github.com/sripwoud/auberge/pull/39))

## [0.1.3](https://github.com/sripwoud/auberge/compare/v0.1.2...v0.1.3) - 2026-01-25

### Added

- add port 853 warning for blocky DNS over TLS ([#38](https://github.com/sripwoud/auberge/pull/38))
- add cross-host restore for VPS migration ([#29](https://github.com/sripwoud/auberge/pull/29))

### Fixed

- replace namecheap warnings with cloudflare instructions ([#35](https://github.com/sripwoud/auberge/pull/35))

### Other

- add retry logic for ansible-galaxy install ([#36](https://github.com/sripwoud/auberge/pull/36))
- use `cargo-nextest` for tests ([#28](https://github.com/sripwoud/auberge/pull/28))

### Added

- *(backup)* cross-host restore with `--from-host` flag for VPS migration and disaster recovery
  - Pre-flight validation (SSH connectivity, service existence, disk space)
  - Hostname confirmation prompt to prevent accidental cross-host restores
  - Automatic emergency backup of target host before overwrite
  - Post-restore guidance for manual steps (DNS, config regeneration, health checks)

## [0.1.2](https://github.com/sripwoud/auberge/compare/v0.1.1...v0.1.2) - 2026-01-23

### Added

- *(ssh)* add key authorization command ([#27](https://github.com/sripwoud/auberge/pull/27))
- add backup and restore commands for application data ([#25](https://github.com/sripwoud/auberge/pull/25))

### Other

- *(ssh)* document interactive fallback and SSH config import

## [0.1.1](https://github.com/sripwoud/auberge/compare/v0.1.0...v0.1.1) - 2026-01-22

### Added

- *(ansible)* integrate xdg hosts and playbooks with ansible commands
- *(hosts)* add host management with xdg config storage
- xdg base directory and hybrid distribution architecture ([#23](https://github.com/sripwoud/auberge/pull/23))

### Other

- add host management to readme quick start
- add host management section to develop.md
- *(release)* configure release-plz to only release on feat and fix commits
- align config documentation with mise.toml refactor
- *(readme)* update setup instructions for Cloudflare API token

## [0.1.0](https://github.com/sripwoud/auberge/releases/tag/v0.1.0) - 2026-01-22

### Added

- *(ci)* add release-plz workflow for automated releases ([#19](https://github.com/sripwoud/auberge/pull/19))
- migrate DNS provider from Namecheap to Cloudflare ([#17](https://github.com/sripwoud/auberge/pull/17))
- *(dns)* [**breaking**] implement sandbox-by-default with production flag
- *(dns)* add rate limiting to batch DNS operations
- *(dns)* add logging for deserialization failures
- *(apt)* remove apache2 web server to prevent port conflicts
- add short aliases for all commands and subcommands
- *(dns)* add set-all command for batch subdomain creation
- make app subdomains configurable via environment variables
- add namecheap ip whitelisting warning for ssl certificates
- use minijinja to evaluate Jinja2 templates in inventory
- make deployment fully declarative with port handling
- *(bootstrap)* set hostname from inventory name
- add provider firewall warnings for bootstrap
- *(bootstrap)* atomic SSH port + firewall transition pattern
- *(bootstrap)* auto-prompt for password on first connection
- use SSH_PORT env var for secure port configuration
- refactor playbooks into layered architecture with hierarchical tagging ([#12](https://github.com/sripwoud/auberge/pull/12))
- *(dns)* add namecheap DNS management commands ([#8](https://github.com/sripwoud/auberge/pull/8))
- *(cli)* add rust CLI for selfhost management
- *(ssh)* add connection validation to prevent lockouts during port changes
- *(ansible)* implement dual-user security model and improve infrastructure
- *(ansible)* add Navidrome music server role
- *(ansible)* configure freshrss with automatic feed updates
- *(ansible)* add freshrss role for rss feed aggregator
- *(radicale)* add birthday calendar and git versioning
- override remote user in inventory
- add vibecoder playbook ([#2](https://github.com/sripwoud/auberge/pull/2))
- add yourls role
- add wireguard role
- renew blocky lets encrypt certificate
- define blocky role
- deploy calibre with ansible
- deploy webdav server with ansible
- deploy radicale with ansible
- deploy caddy
- *(ansible)* define init playbook to setup VPS
- remove nginx configs
- *(yourls)* convert from nginx to caddy
- *(calibre)* convert from nginx to caddy
- *(radicale)* convert from nginx to caddy
- add adguard home config files
- add wireguard script
- add minflux fly.toml and deply script
- add `radicale` nginx and service config files
- add calibre server nginx and service files

### Fixed

- *(caddy)* use 127.0.0.1 instead of localhost for reverse proxy
- *(dns)* remove double-fetch bug in set_a_record
- add radicale and webdav password variables to group_vars
- *(radicale)* use admin_user_name instead of undefined user_name
- *(navidrome)* add admin user to navidrome group for sync access
- *(navidrome)* let deb package create user and group
- *(navidrome)* create music folder after user creation
- standardize app subdomains to use primary_domain variable
- *(blocky)* add missing reload systemd handler
- *(caddy)* update apt cache after adding xcaddy repository
- *(caddy)* replace deprecated apt_key with get_url for GPG key
- *(infrastructure)* pass user variables to bash role
- *(inventory)* add IdentitiesOnly=yes to prevent too many auth failures
- *(inventory)* accept ansible_port as string or u16 for flexible parsing
- *(apt)* use modern repository management without apt-key
- *(ssh)* validate with ansible user key and define ssh_port globally
- *(bootstrap)* ignore known_hosts during initial password connection
- *(bootstrap)* disable strict host checking for initial password connection
- *(ssh)* remove keyname prompt and use consistent key path pattern
- *(user)* use ADMIN_USER_NAME env var instead of prompting
- *(ansible_user)* create sshusers group before adding users to it
- run ansible-playbook from ansible/ dir to find ansible.cfg
- disable vault password prompt since using mise secrets
- *(navidrome)* proper music upload permissions
- *(ansible)* add SSH key path for lechuck-cloud connection
- *(freshrss)* add PHP sessions directory to ReadWritePaths
- *(freshrss)* handle local modifications in git repository
- *(caddy)* create log directory with proper permissions
- *(blocky)* fix lego renewal script issues
- *(blocky)* set proper ownership for lego binary
- update vibecoder playbook
- update roles path

### Other

- update mise tasks
- *(deps)* update namecheap to fix IsSuccess validation
- *(deps)* update namecheap to fix boolean deserialization
- remove unused code to eliminate warnings
- add NAMECHEAP_CLIENT_IP secret
- *(deps)* update namecheap fork to latest commit
- *(deps)* switch to namecheap fork with API error visibility fix
- *(sync)* use ansible user instead of admin for music sync
- *(dns)* dynamically discover subdomains from env vars
- remove unused email field from UserConfig
- add mise tasks for building and running auberge CLI
- simplify inventory and rename hosts to auberge
- streamline local dev config to single source of truth ([#14](https://github.com/sripwoud/auberge/pull/14))
- mv mise config to root
- update hk config
- parameterize user config for public repository ([#11](https://github.com/sripwoud/auberge/pull/11))
- rename main to master
- rename to `auberge` and reorganize as CLI-first structure ([#10](https://github.com/sripwoud/auberge/pull/10))
- lint ([#9](https://github.com/sripwoud/auberge/pull/9))
- remove mise ansible tasks
- format ssh validation yaml
- format and add wireguard to selfhosted playbook
- *(navidrome)* use .deb package instead of manual binary
- add .claude to gitignore
- migrate mise config from .config/mise to .mise
- edit vibecoder debug exit message
- lint
- wip
- add convco to mise tools
- restructure repo ([#3](https://github.com/sripwoud/auberge/pull/3))
- setup hk and gh actions ([#1](https://github.com/sripwoud/auberge/pull/1))
- remove unused
- define ansible roles
- reduce miniflux machine memory
- update miniflux deploy task
