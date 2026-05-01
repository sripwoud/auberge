# Declarative YAML backup recipes, not a Rust trait

A Backup Recipe is data — a `backup:` section in a Playbook Meta YAML file — interpreted by a generic Recipe Executor. It is not a `BackupRecipe` Rust trait with one impl per app.

## Considered Options

- **Rust trait `BackupRecipe` with one impl per app.** Rejected: the trait-elegance pull is real but misdirected. An audit of the existing 2,635-line `commands/backup.rs` found _zero_ conditional or parsed-output logic across all ~9 apps — every recipe is "stop services → optional DB dump → rsync paths → optional DB restore → start services." That's data, not behavior. The only app-specific quirk (paperless DB migration after restore) fits a `post_restore_command:` field. A trait would create N small files of stringified shell commands and brittle "did you call run() with this string" tests, while leaving the real bug surface (does the shell command work on the target?) to integration tests either way.
- **Hybrid (declarative default + Rust escape hatch).** Rejected: starting with two ways to write a recipe means the declarative path never gets a fair try. If a future app genuinely needs imperative logic, the cost of adding an escape hatch _then_ is small.

## Why this is non-obvious

A Rust developer's reflex when seeing per-app variation is "extract a trait." This ADR records that the variation here is in _content_, not _shape_ — and that the audit was done. Future architecture reviews should not re-suggest the trait.
