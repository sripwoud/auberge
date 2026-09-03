//! ADR-0004's `-o, --output {human,json}` is declared once, and every command
//! that carries it reaches that one declaration through `#[command(flatten)]`.
//!
//! `main.rs` fences the *surface* — which commands carry the flag, how it is
//! spelled, where it sits in `--help`. It cannot see how that surface is
//! produced: fifteen re-pasted `#[arg(...)]` blocks and one flattened struct
//! build the identical clap tree, so the fence over the tree stays green while
//! the duplication #818 removed grows back. This is the question about source
//! as text, so it is asked here (ADR-0046).
//!
//! The scan would empty out if the declaration were reworded, so the witness
//! that it still reaches the real one is an assertion too.

mod crate_source;

use crate_source::modules;

/// The one line only ADR-0004's flag declares. A re-paste has to carry it: drop
/// the default and the command stops defaulting to `human`, which `main.rs`
/// fences.
const DECLARATION: &str = r#"default_value = "human""#;

/// Where that line is allowed to be.
const HOME: &str = "src/output.rs";

#[test]
fn the_output_flag_is_declared_in_one_place() {
    let walked = modules();
    let offenders: Vec<&str> = walked
        .iter()
        .filter(|module| module.repo_relative != HOME && module.source.contains(DECLARATION))
        .map(|module| module.repo_relative.as_str())
        .collect();

    assert!(
        offenders.is_empty(),
        "these modules declare `--output` themselves instead of flattening \
         `OutputArg` from {HOME} (#818):\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn the_one_place_is_still_where_the_scan_looks() {
    let home = modules()
        .into_iter()
        .find(|module| module.repo_relative == HOME)
        .unwrap_or_else(|| panic!("the crate walk no longer reaches {HOME}"));

    assert!(
        home.source.contains(DECLARATION),
        "{HOME} no longer spells `{DECLARATION}` — the scan above now passes \
         vacuously, and every re-pasted copy with it"
    );
}
