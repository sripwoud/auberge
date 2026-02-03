# Changelog

Version history and release notes.

For detailed changes, see [GitHub Releases](https://github.com/sripwoud/auberge/releases).

## v0.2.0 (2026-01-XX)

**Breaking Changes:**

- Reorganized backup structure to timestamp-first for better organization

**Features:**

- YOURLS URL shortener support
- Improved backup/restore safety checks

**Improvements:**

- Better error messages
- Harden workflow permissions

## v0.1.4 (2025-XX-XX)

**Features:**

- YOURLS automated deployment with database initialization

## v0.1.3 (Earlier)

**Features:**

- Initial backup and restore functionality
- Core application deployments
- DNS management via Cloudflare

## Previous Versions

For earlier versions, see [GitHub Releases](https://github.com/sripwoud/auberge/releases).

## Versioning

Auberge follows [Semantic Versioning](https://semver.org):

- **Major (X.0.0):** Breaking changes
- **Minor (0.X.0):** New features, backwards compatible
- **Patch (0.0.X):** Bug fixes, backwards compatible

## Release Process

Releases are automated via GitHub Actions:

1. PR merged to main
2. [release-plz](https://release-plz.ieni.dev/) creates release PR
3. Maintainer reviews and merges
4. GitHub Action publishes to crates.io
5. Release notes generated from conventional commits

## Upgrade Guide

### From v0.1.x to v0.2.x

**Backup structure change:**

Old structure:

```
backups/
└── my-vps/
    └── baikal/
        ├── latest/
        └── 2026-01-23_14-30-00/
```

New structure:

```
backups/
└── my-vps/
    └── baikal/
        ├── 2026-01-23_14-30-00/
        └── latest -> 2026-01-23_14-30-00
```

**Action required:** None - old backups still work, new backups use new structure.

## Related Pages

- [Roadmap](about/roadmap.md) - Future plans
- [GitHub Releases](https://github.com/sripwoud/auberge/releases) - Detailed changelog
