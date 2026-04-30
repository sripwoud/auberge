# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — the project's domain glossary.
- **`meta/adr.md`** — the curated overview of foundational architectural decisions (no Docker, Rust CLI, Ansible, etc.).
- **`meta/adr/`** — granular per-decision ADR files added over time. Read the ones touching the area you're about to work in.

> ADRs live under `meta/`, not the canonical `docs/adr/`. The `docs/` directory is owned by the docsify site for the auberge CLI; internal architectural decisions stay outside it.

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The producer skill (`/grill-with-docs`) creates them lazily when terms or decisions actually get resolved.

## File structure

Single-context repo:

```
/
├── CONTEXT.md
├── meta/
│   ├── adr.md            ← curated overview of foundational decisions
│   ├── adr/              ← granular per-decision ADRs (created lazily)
│   │   └── 0001-…md
│   ├── agents/           ← agent skill config (this folder)
│   └── roadmap.md
├── docs/                 ← docsify site for auberge CLI (not for ADRs)
└── src/
```

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal — either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/grill-with-docs`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (xyz) — but worth reopening because…_
