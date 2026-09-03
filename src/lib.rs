//! Every module the CLI is built from, as a library the integration fences can
//! import.
//!
//! Cargo links an integration test against the *library* target, and there was
//! none, so a fence needing a crate type had to retype it. ADR-0046 has the
//! incident that cost and the rule it bought: a fence asserting about a crate
//! item reaches it with `use`.
//!
//! Everything is `pub`, and deliberately not a curated surface. Nothing outside
//! this repository consumes the crate, so a narrower `pub` list would be a
//! second interface maintained against Cargo's visibility rules for no reader —
//! the discipline that matters is which vocabulary the fences actually import.
//! The cost is real and named in the ADR: `dead_code` no longer reaches a `pub`
//! item, so an unused `pub fn` is nobody's warning. The 16 `#[allow(dead_code)]`
//! sites that used to mark that silence are gone (#816). Every one was inert,
//! and each also read as a claim that the item under it had a reader somewhere
//! that a lint was in the way of. What nothing read is deleted; what a test
//! alone reads carries `#[cfg(test)]`; three of them — `KeyRegistry::get`,
//! `KeyRegistry::iter`, `Progress::warn` — had ordinary production readers the
//! whole time, which is the cost of a badge nobody has to keep true.
//!
//! `main.rs` holds the clap tree, the global output flags, the dispatch `match`,
//! and the tests over that clap surface.

pub mod ansible_assets;
pub mod commands;
pub mod config;
pub mod hosts;
pub mod key_registry;
pub mod output;
pub mod playbook_meta;
pub mod prompt;
pub mod services;
pub mod signal;
pub mod ssh_config;
pub mod tool_versions;
