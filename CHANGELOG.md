# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
