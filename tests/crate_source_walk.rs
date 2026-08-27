//! Fence on the shared crate-source walk the two `src/` fences read through.
//!
//! `assets_guard` and `vendor_types_stay_in_adapter` both answer their question
//! by walking `src/**/*.rs`. The walk is the shared premise underneath them, so
//! a walk that quietly stops reaching somewhere does not fail: it shrinks the
//! domain, and every fence over it passes vacuously (#679). These are the
//! walk's own assertions — `tests/task_walker.rs` for the ansible tree, one
//! layer over.
//!
//! Anchored on the crate itself where the claim is about the crate, and on a
//! temporary directory where it is a postcondition of the walk. Ordering and
//! the file filter are the second kind: `read_dir` returns whatever the
//! filesystem holds, and this checkout was written by git in index order, so
//! every directory under `src/` happens to come back sorted and the tree cannot
//! falsify either claim.

mod crate_source;

use crate_source::{CRATE_MODULES, modules, rust_files};

/// The reach pin, under a name again. It fires inside [`modules`] so that every
/// fence inherits it rather than trusting the walk, which means its failure
/// surfaces in whichever fence happened to call first; this is the assertion
/// that says so out loud, and that the pin is not vacuous — a `CRATE_MODULES`
/// emptied to `&[]` would make the two-way difference pass over nothing.
#[test]
fn test_the_walk_is_pinned_to_the_crate_it_walks() {
    assert!(
        !CRATE_MODULES.is_empty(),
        "CRATE_MODULES is empty, so the reach pin compares two empty sets and \
         every fence over src/ passes having seen nothing"
    );
    assert_eq!(
        modules().len(),
        CRATE_MODULES.len(),
        "the walk and its declared reach disagree on how many modules the crate holds"
    );
}

/// The two spellings were one name, `relative`, in two files meaning two
/// things, until #675 renamed one of them (#679). Distinct names are a compile
/// time fact; that each still produces the form its name claims is not, and a
/// fence keying `DECLARED_TRANSIENT` on one or `CONFINED_VENDORS` on the other
/// is reading whichever it was handed.
#[test]
fn test_the_two_spellings_name_the_same_file_and_stay_distinct() {
    let walked = modules();
    assert!(!walked.is_empty(), "the walk reached no module at all");

    for module in &walked {
        assert_eq!(
            module.repo_relative,
            format!("src/{}", module.src_relative),
            "the two spellings of one module disagree about which file it is"
        );
        assert!(
            !module.src_relative.starts_with("src/"),
            "{} is src-relative in name only — it still carries the src/ prefix, \
             so a fence keying on it reads the repo-relative form under the \
             wrong name",
            module.src_relative
        );
    }
}

/// Eager reads exist so an unreadable file fails once, in the walk, rather than
/// once per call site. That is only worth anything if the text a `Module`
/// carries is the text on disk: a source silently emptied would leave every
/// scan over it finding nothing and passing.
#[test]
fn test_every_module_carries_the_text_on_disk() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    for module in modules() {
        let on_disk = std::fs::read_to_string(root.join(&module.repo_relative))
            .unwrap_or_else(|e| panic!("{} must be readable: {e}", module.repo_relative));
        assert_eq!(
            module.source, on_disk,
            "{} carries text the file does not",
            module.repo_relative
        );
    }
}

/// Whole path, not basename: a nested file sorts under its own directory rather
/// than among the files beside it, so `nested/bravo.rs` follows `mike.rs`
/// instead of `alpha.rs`.
#[test]
fn test_the_walk_reports_in_path_order_whatever_the_filesystem_says() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["zulu.rs", "mike.rs", "alpha.rs"] {
        std::fs::write(dir.path().join(name), "").unwrap();
    }
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested/bravo.rs"), "").unwrap();

    let found: Vec<String> = rust_files(dir.path())
        .iter()
        .map(|path| {
            path.strip_prefix(dir.path())
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert_eq!(found, ["alpha.rs", "mike.rs", "nested/bravo.rs", "zulu.rs"]);
}

/// The filter and the descent, on a tree that can falsify both. `src/` holds no
/// non-Rust file at all today, so nothing in the crate distinguishes a walk that
/// filters by extension from one that takes everything it finds.
#[test]
fn test_the_walk_takes_rust_files_at_any_depth_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("kept.rs"), "").unwrap();
    std::fs::write(dir.path().join("skipped.md"), "").unwrap();
    std::fs::write(dir.path().join("skipped"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("one/two")).unwrap();
    std::fs::write(dir.path().join("one/two/deep.rs"), "").unwrap();
    std::fs::write(dir.path().join("one/two/deep.rs.bak"), "").unwrap();

    let found: Vec<String> = rust_files(dir.path())
        .iter()
        .map(|path| {
            path.strip_prefix(dir.path())
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert_eq!(found, ["kept.rs", "one/two/deep.rs"]);
}
