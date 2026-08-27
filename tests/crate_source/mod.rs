//! The crate's own source, as the fences read it.
//!
//! Two fences ask a question of `src/**/*.rs` — which call sites discard an
//! `AnsibleAssets` guard, which modules name a vendor crate — and each carried
//! its own copy of the walk that answers it. The walk is the shared premise
//! underneath both, and a premise that quietly stops reaching somewhere does
//! not fail. It shrinks the domain, and every fence over it goes on passing,
//! vacuously. `tests/common/mod.rs` is the same lesson learned on the ansible
//! tree, where six copies had already diverged far enough that two fences
//! asking the same question of the same tree got answers 83 tasks apart (#654).
//! That module is scoped to `ansible/`; this one is the crate's own source, so
//! it stands beside it rather than inside it.
//!
//! Only one of the two copies pinned its reach: `vendor_types_stay_in_adapter`
//! asserted the walked set equals [`CRATE_MODULES`] by difference in both
//! directions, and `assets_guard` walked and trusted. The pin now sits inside
//! [`modules`], so it is inherited rather than re-derived — every fence reading
//! the crate this way fails the moment the walk stops seeing a module the crate
//! holds (#679).
//!
//! The two copies also relativised a path differently, and the difference was a
//! property of whichever copy you happened to be reading: `assets_guard`
//! stripped `src/` and the vendor fence stripped the manifest directory, both
//! behind a function named `relative` until #675 renamed one of them. Both
//! spellings are now named products of the one walk — [`Module::repo_relative`]
//! and [`Module::src_relative`] — so a fence asks for the form it wants and
//! cannot reach the other one under the same name.

// Neither fence reads every field, so each sees the rest as dead. The cost of
// the blanket allow is the usual one: a field no fence reads at all is never
// flagged either. Both are load-bearing in a `DECLARED_TRANSIENT` key and a
// `CONFINED_VENDORS` key respectively, so a field that stopped being produced
// would fail those comparisons rather than pass quietly.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Every module in the crate, so the walk is checked against a set rather than
/// trusted to have found things.
///
/// Spelled repo-relative because that is the form a reader can paste into an
/// editor, and the form `CONFINED_VENDORS` already names an adapter by.
const CRATE_MODULES: &[&str] = &[
    "src/ansible_assets.rs",
    "src/commands.rs",
    "src/commands/ansible.rs",
    "src/commands/backup.rs",
    "src/commands/bichon/mod.rs",
    "src/commands/bichon/reconcile.rs",
    "src/commands/bichon/rescan.rs",
    "src/commands/bichon/verify.rs",
    "src/commands/config_cmd.rs",
    "src/commands/deploy.rs",
    "src/commands/dns.rs",
    "src/commands/headscale.rs",
    "src/commands/host.rs",
    "src/commands/select.rs",
    "src/commands/ssh.rs",
    "src/commands/sync.rs",
    "src/commands/versions.rs",
    "src/config.rs",
    "src/hosts.rs",
    "src/key_registry.rs",
    "src/main.rs",
    "src/output.rs",
    "src/playbook_meta.rs",
    "src/prompt.rs",
    "src/services.rs",
    "src/services/ansible_runner.rs",
    "src/services/backup.rs",
    "src/services/backup/executor.rs",
    "src/services/backup/recipe.rs",
    "src/services/backup/restic.rs",
    "src/services/backup/session.rs",
    "src/services/backup/verify.rs",
    "src/services/bichon/api.rs",
    "src/services/bichon/coverage.rs",
    "src/services/bichon/folder_filter.rs",
    "src/services/bichon/mod.rs",
    "src/services/bichon/rescan.rs",
    "src/services/cloudflare_dns.rs",
    "src/services/dependency_resolver.rs",
    "src/services/dns.rs",
    "src/services/dns_verify.rs",
    "src/services/inventory.rs",
    "src/services/progress.rs",
    "src/services/rsync.rs",
    "src/services/ssh.rs",
    "src/services/ssh_include.rs",
    "src/services/unit_state.rs",
    "src/signal.rs",
    "src/ssh_config.rs",
    "src/ssh_session.rs",
    "src/tool_versions.rs",
];

/// One `.rs` file under `src/`: its text, and both spellings of its name.
///
/// The text is read eagerly because both fences read every module anyway, and
/// a `Module` that carried a path instead would put the failure message for an
/// unreadable file back at each call site, where the two copies already spelled
/// it two ways.
pub struct Module {
    /// `src/services/dns.rs` — rooted at the repository, so it is a path a
    /// reader can open and the form `CONFINED_VENDORS` names an adapter by.
    pub repo_relative: String,
    /// `services/dns.rs` — rooted at `src/`, the form `DECLARED_TRANSIENT`
    /// names a call site by.
    pub src_relative: String,
    pub source: String,
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn src_dir() -> PathBuf {
    repo().join("src")
}

/// `path` with `root` stripped, forward-slashed. The one relativiser; which of
/// [`Module`]'s two names it produces is `root`'s to say, so the difference
/// between them is a parameter rather than a property of a second copy.
fn strip(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or_else(|e| panic!("{} must live under {}: {e}", path.display(), root.display()))
        .to_string_lossy()
        .replace('\\', "/")
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The walk's reach, by equality in both directions: a new module fails until
/// it is listed, and a listing the crate no longer holds fails until it is
/// removed. A floor (`>= 45`) would let modules leave the walk silently, and
/// every fence over this domain can pass by seeing nothing.
fn pin_reach(walked: &[Module]) {
    let seen: BTreeSet<&str> = walked
        .iter()
        .map(|module| module.repo_relative.as_str())
        .collect();
    let listed: BTreeSet<&str> = CRATE_MODULES.iter().copied().collect();

    let unlisted: Vec<&&str> = seen.difference(&listed).collect();
    let missing: Vec<&&str> = listed.difference(&seen).collect();
    assert!(
        unlisted.is_empty() && missing.is_empty(),
        "the crate's module set moved.\n  new, add to CRATE_MODULES in tests/crate_source/mod.rs: {unlisted:?}\n  gone, drop from CRATE_MODULES: {missing:?}"
    );
}

/// Every `.rs` file under `src/`, at any depth, sorted, with its reach pinned
/// against [`CRATE_MODULES`].
///
/// The pin runs on every call rather than living in a fence of its own, so a
/// walk that stopped reaching somewhere fails inside whichever fence relies on
/// it — the caller inherits the reach instead of trusting it.
pub fn modules() -> Vec<Module> {
    let mut paths = Vec::new();
    rust_files(&src_dir(), &mut paths);
    paths.sort();

    let walked: Vec<Module> = paths
        .iter()
        .map(|path| Module {
            repo_relative: strip(path, &repo()),
            src_relative: strip(path, &src_dir()),
            source: fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display())),
        })
        .collect();

    pin_reach(&walked);
    walked
}
