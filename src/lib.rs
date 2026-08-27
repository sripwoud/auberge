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
//! item, so the ~18 `#[allow(dead_code)]` sites in this crate are now inert and
//! an unused `pub fn` is nobody's warning.
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
pub mod ssh_session;
pub mod tool_versions;
