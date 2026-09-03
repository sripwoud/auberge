//! ADR-0046 traded `dead_code` on `pub` items for a library target the fences
//! can import, and #816 swept the 16 `#[allow(dead_code)]` sites that had
//! accumulated in the blind spot it opened. This is what keeps them from
//! coming back.
//!
//! Narrow by construction: it does not find dead code. The compiler cannot,
//! here — that is the cost the ADR records and accepts. What it asserts is that
//! nobody reaches for the attribute that used to paper the silence over, which
//! is the signal #816 found the whole class by.
//!
//! #817 offered to close itself as `wontfix` if that sweep read as one-time
//! cleanup rather than a standing risk. It does not. Every one of the 16 was
//! inert, and each also read as a claim that the item under it had a reader
//! somewhere that a lint was in the way of — and three (`KeyRegistry::get`,
//! `KeyRegistry::iter`, `Progress::warn`) had ordinary production readers the
//! whole time. A badge nobody has to keep true does not decay into a wrong
//! badge once; it was already wrong on three of sixteen when anybody looked.
//!
//! Reads `src/` through `crate_source` rather than walking it again, so the
//! reach pin inside [`modules`] is inherited: a walk that stops seeing a
//! module fails here instead of shrinking this scan's domain to something it
//! passes over vacuously (#679).
//!
//! Scoped to `src/` rather than the repository, and the boundary is a real
//! difference rather than a convenience: an integration test compiles as its
//! own crate with a generated root, so `dead_code` *does* reach items there.
//! `tests/crate_source/mod.rs` carries the attribute on `Module`'s two path
//! fields for exactly that reason — each fence reads the spelling it needs and
//! the compiler flags the other as unread, once per test binary.
//!
//! `#[cfg(test)]` is the sanctioned way to say "a test alone reads this", and
//! it is not fenced — it says the true thing an allow said falsely.
//!
//! What it does not see, because [`code_lines`] cuts whole-line `//` comments
//! and nothing else: an attribute trailing a code line, and a `/* */` block.
//! Neither is reachable under `cargo fmt`, which puts an attribute on its own
//! line, so the gap is rustfmt's to keep shut rather than this fence's to
//! parse for.

mod crate_source;

use crate_source::{code_lines, modules};

/// The `dead_code` lint, matched as the bare lint name.
///
/// The name is the invariant; the attribute around it is not. `expect` is
/// stable and is what rustc's own suggestion reaches for, an inner
/// `#![allow(..)]` suppresses a whole module, `cfg_attr` hides either behind a
/// predicate, and a multi-lint list has no closing paren after the name. One
/// substring covers all of them, and covers a `cargo fmt`-split list besides,
/// where the lint name ends up on a line of its own. Matching
/// `allow(dead_code)` instead would miss five shapes, one of them the shape a
/// compiler suggestion produces.
///
/// Safe bare because no item in the crate is *named* `dead_code`: the three
/// live mentions under `src/` are prose, which [`code_lines`] cuts.
const DEAD_CODE: &str = "dead_code";

/// The `unused` lint *group*, which contains `dead_code` — the same paper under
/// another name.
///
/// This one cannot be matched bare. `sweep_unused` and `collect_unused` in
/// `ansible_assets.rs` are real functions, and `required_keys.rs` passes a
/// literal `"unused"` to a fixture, so a substring match fires on production
/// code and a whole-word match still fires on the fixture. Hence
/// [`attribute_names_unused_group`], which asks for the token inside an
/// attribute. The one shape that escapes it is a `cargo fmt`-split list, where
/// `unused,` sits on a line carrying no attribute opener — the asymmetry with
/// [`DEAD_CODE`] is the price of a lint name that collides with real
/// identifiers, and it is stated rather than parsed around (ADR-0046).
const UNUSED_GROUP: &str = "unused";

/// `true` when `line` opens an attribute and names the `unused` lint group as
/// its own token.
///
/// The attribute gate is what makes the token match usable: every `unused` in
/// the crate's real code sits on a line with no `#[`. The token split is what
/// keeps `unused_imports` and `unused_variables` — specific lints that do not
/// reach `dead_code` — from being read as the group.
fn attribute_names_unused_group(line: &str) -> bool {
    if !(line.contains("#[") || line.contains("#![")) {
        return false;
    }
    line.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|token| token == UNUSED_GROUP)
}

/// Lines of `source` outside `//` comments that switch the `dead_code` lint
/// off, as `(0-based index, lint named)`.
///
/// Comment-stripped because the prose naming what is forbidden must not trip
/// the rule: `src/lib.rs` and `src/hosts.rs` both discuss `dead_code` in doc
/// comments today, and a fence that fires on its own explanation teaches people
/// to delete the explanation.
fn suppressions_in(source: &str) -> Vec<(usize, &'static str)> {
    code_lines(source)
        .filter_map(|(index, line)| {
            if line.contains(DEAD_CODE) {
                Some((index, DEAD_CODE))
            } else if attribute_names_unused_group(line) {
                Some((index, UNUSED_GROUP))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn no_module_suppresses_the_dead_code_lint() {
    let mut offenders: Vec<String> = Vec::new();
    for module in modules() {
        for (index, lint) in suppressions_in(&module.source) {
            offenders.push(format!(
                "  {}:{} suppresses `{lint}`",
                module.repo_relative,
                index + 1
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "a suppression attribute is papering over an unread item:\n{}\n\
         On a `pub` item it silences nothing at all — `dead_code` does not \
         reach one in this crate (ADR-0046) — so it only claims a reader that \
         need not exist. On a private item it silences a warning that is \
         telling the truth. The fix is the same either way: delete what \
         nothing reads, and mark what only a test reads `#[cfg(test)]` \
         (#816, #817).",
        offenders.join("\n")
    );
}

/// Without this the scan above passes by finding nothing whether or not it can
/// see anything — the same failure mode `crate_source`'s reach pin covers one
/// level up. Reach proves the files were read; this proves the reading would
/// notice, in every shape the two constants claim to cover.
///
/// Spelled as inputs and expectations rather than derived from `SUPPRESSIONS`,
/// because a witness computed from the thing it witnesses asserts only that the
/// matcher agrees with itself.
#[test]
fn the_scan_finds_every_shape_it_claims_to() {
    let shapes: &[(&str, &str)] = &[
        ("#[allow(dead_code)]", DEAD_CODE),
        ("#[expect(dead_code)]", DEAD_CODE),
        ("#![allow(dead_code)]", DEAD_CODE),
        ("#[allow(dead_code, unused_imports)]", DEAD_CODE),
        ("#[cfg_attr(test, allow(dead_code))]", DEAD_CODE),
        ("    dead_code,", DEAD_CODE),
        ("#[allow(unused)]", UNUSED_GROUP),
        ("#[expect(unused)]", UNUSED_GROUP),
        ("#![allow(unused)]", UNUSED_GROUP),
        ("#[allow(unused, unused_imports)]", UNUSED_GROUP),
        ("#[allow(unused_imports, unused)]", UNUSED_GROUP),
        ("#[allow(clippy::redundant_clone, unused)]", UNUSED_GROUP),
        ("#[cfg_attr(test, allow(unused))]", UNUSED_GROUP),
    ];

    for (source, lint) in shapes {
        assert_eq!(
            suppressions_in(source),
            vec![(0, *lint)],
            "{source} must be read as suppressing {lint}"
        );
    }
}

/// The shapes the scan must pass over, each of them live in the tree it scans:
/// prose that names the attribute, the `unused` prefix and the bare word on
/// real identifiers and literals, and the specific `unused_*` lints, which do
/// not reach `dead_code`. Any one of them firing would make this fence a reason
/// to reword a doc comment or rename a function.
#[test]
fn the_scan_passes_over_prose_identifiers_and_narrower_lints() {
    let benign: &[&str] = &[
        "//! The 16 `#[allow(dead_code)]` sites that marked that silence are gone (#816).",
        "/// with no production call site no longer trips `dead_code` (ADR-0046)",
        "// #[allow(unused)]",
        "fn sweep_unused(root: &Path, fingerprint: &str) {",
        "    if let Err(err) = collect_unused(root, fingerprint) {",
        "        let dir = fixture_ansible_dir(&[\"unused\"], &[]);",
        "            \"http://unused\".to_string(),",
        "#[cfg(test)]",
        "#[allow(clippy::too_many_arguments)]",
        "#[allow(async_fn_in_trait)]",
        "#[allow(unused_variables)]",
        "#[allow(unused_imports)]",
    ];

    for line in benign {
        assert_eq!(
            suppressions_in(line),
            vec![],
            "{line} must not be read as a dead_code suppression"
        );
    }
}
