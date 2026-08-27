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
//!
//! `removed_unit_failed_state.rs` is the third file in the suite that reads
//! crate source, and is deliberately not one of them. It reads exactly one
//! file, `src/playbook_meta.rs`, to mirror the unit-type list declared there
//! (#656) — it carries no walk, so there is nothing here to fold, and routing
//! one `read_to_string` through a walk of 51 modules would buy it nothing.
//! Named so its absence does not read as coverage: it reads that file beside
//! the walk rather than through it, and a `src/` the walk stopped reaching
//! would not fail it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Every module in the crate, so the walk is checked against a set rather than
/// trusted to have found things.
///
/// Spelled repo-relative because that is the form a reader can paste into an
/// editor, and the form `CONFINED_VENDORS` already names an adapter by.
pub const CRATE_MODULES: &[&str] = &[
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
/// The text is read eagerly so that an unreadable file has one failure message
/// instead of one per call site — the two copies this replaced already spelled
/// that message two ways. It is a real cost at the two call sites that want a
/// single module and discard the other fifty reads, and it is paid on purpose:
/// those two used to reach their file with a `read_to_string` beside the walk,
/// which is how a fence comes to assert against a file its own scan never saw.
pub struct Module {
    /// `src/services/dns.rs` — rooted at the repository, so it is a path a
    /// reader can open and the form `CONFINED_VENDORS` names an adapter by.
    ///
    /// Allowed dead because a fence reads the spelling it needs and no other:
    /// this one is dead in `assets_guard`'s binary and `src_relative` is dead
    /// in the vendor fence's. The allow is on the two fields rather than the
    /// module, so a *helper* here that no fence calls at all is still flagged —
    /// which is the cost `common/mod.rs` pays for its blanket allow and offsets
    /// with `tests/task_walker.rs`. `tests/crate_source_walk.rs` is this
    /// module's equivalent, and reads both spellings besides.
    #[allow(dead_code)]
    pub repo_relative: String,
    /// `services/dns.rs` — rooted at `src/`, the form `DECLARED_TRANSIENT`
    /// names a call site by.
    #[allow(dead_code)]
    pub src_relative: String,
    pub source: String,
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn src_dir() -> PathBuf {
    repo().join("src")
}

/// `path` with `root` stripped, forward-slashed. The one relativiser: which of
/// [`Module`]'s two names it produces is `root`'s to say, so the difference
/// between them is a parameter rather than a property of a second copy.
fn relative_to(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or_else(|e| panic!("{} must live under {}: {e}", path.display(), root.display()))
        .to_string_lossy()
        .replace('\\', "/")
}

fn collect_rust(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Every `.rs` file under `dir`, at any depth, in path order.
///
/// Sorted here rather than at the caller because ordering is a postcondition of
/// the walk, not a fact about the tree: `read_dir` returns whatever the
/// filesystem holds, and this checkout happens to come back sorted already.
/// `tests/crate_source_walk.rs` builds a directory in the opposite order, which
/// is what makes the claim falsifiable.
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_rust(dir, &mut found);
    found.sort();
    found
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

    let unlisted: Vec<&str> = seen.difference(&listed).copied().collect();
    let missing: Vec<&str> = listed.difference(&seen).copied().collect();
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
    let walked: Vec<Module> = rust_files(&src_dir())
        .iter()
        .map(|path| Module {
            repo_relative: relative_to(path, &repo()),
            src_relative: relative_to(path, &src_dir()),
            source: fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display())),
        })
        .collect();

    pin_reach(&walked);
    walked
}
