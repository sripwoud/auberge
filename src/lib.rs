//! Every module the CLI is built from, as a library the integration fences can
//! import.
//!
//! The crate shipped as a bin target alone, which gave `tests/` no way to `use`
//! anything in it: Cargo links an integration test against the *library*
//! target, so a fence needing a crate type had to retype it. Both fences over
//! systemd unit names did — `unit_ownership` mirrored the unit-type table and
//! `qualified_unit_name` by hand, and `removed_unit_failed_state` held its own
//! mirror true by reading `src/playbook_meta.rs` as *text* and string-splitting
//! on Rust syntax to recover one `const`. A mirror is a second authority: #653
//! caught this one admitting five of systemd's eleven unit types, with a green
//! build saying otherwise (#667).
//!
//! Everything is `pub`, and deliberately not a curated surface. Nothing outside
//! this repository consumes the crate, so a narrower `pub` list would be a
//! second interface maintained against Cargo's visibility rules for no reader —
//! the discipline that matters is which vocabulary the fences actually import.
//!
//! `main.rs` keeps only what a library cannot hold: the clap tree, the global
//! output flags, and the dispatch `match`.
//!
//! ADR-0046.

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
