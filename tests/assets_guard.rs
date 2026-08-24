use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The one site whose guard has to outlive a subprocess: `run_playbook` and
/// `run_bootstrap` spawn `ansible-playbook`, which reads templates and
/// `include_*` files lazily, at task runtime. Its `AnsibleAssets` must stay
/// bound for the whole function so the shared lock on the tree is still held
/// while the child reads from it.
const RUNNER: &str = "services/ansible_runner.rs";

/// Sites that discard the guard in the same expression, keeping only a path.
/// The lock is released immediately, so a concurrent invocation carrying a
/// different fingerprint may sweep the tree out from under the path. Declared,
/// not tolerated by accident: every one of these reads a file straight away, so
/// the worst outcome is a missing file and a hard error — never a play compiled
/// from one tree and rendered from another, which is what #628 was.
///
/// A new entry here needs the same argument. Anything that hands the path to a
/// child process, or holds it across user interaction, belongs in the runner's
/// shape instead: bind the `AnsibleAssets`, then use its paths.
const DECLARED_TRANSIENT: &[(&str, usize)] = &[
    ("commands/deploy.rs", 1),
    ("services/backup/recipe.rs", 1),
    ("services/dependency_resolver.rs", 7),
    ("services/inventory.rs", 1),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src must be readable").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn transient_uses(source: &str) -> usize {
    source
        .match_indices("AnsibleAssets::prepare()")
        .filter(|&(index, matched)| {
            let rest = &source[index + matched.len()..];
            rest.starts_with("?.") || rest.starts_with(".unwrap().")
        })
        .count()
}

fn relative(path: &Path) -> String {
    path.strip_prefix(src_dir())
        .expect("every scanned file lives under src")
        .to_string_lossy()
        .replace('\\', "/")
}

fn scan() -> BTreeMap<String, usize> {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    files
        .iter()
        .map(|path| {
            let source = fs::read_to_string(path).expect("source must be readable");
            (relative(path), transient_uses(&source))
        })
        .filter(|(_, count)| *count > 0)
        .collect()
}

#[test]
fn test_only_declared_call_sites_drop_the_assets_guard() {
    let declared: BTreeMap<String, usize> = DECLARED_TRANSIENT
        .iter()
        .map(|(file, count)| ((*file).to_string(), *count))
        .collect();

    assert_eq!(
        scan(),
        declared,
        "a call site that discards its AnsibleAssets keeps only a path a concurrent \
         sweep can delete; bind the assets for as long as the path is used, or add \
         the site to DECLARED_TRANSIENT with the argument for why the window is safe"
    );
}

#[test]
fn test_the_playbook_runner_holds_its_guard() {
    let source = fs::read_to_string(src_dir().join(RUNNER)).expect("the runner must exist");

    assert!(
        source.contains("AnsibleAssets::prepare()"),
        "{RUNNER} must prepare the assets it runs ansible-playbook against"
    );
    assert_eq!(
        transient_uses(&source),
        0,
        "{RUNNER} spawns ansible-playbook, which reads templates lazily at task \
         runtime; discarding the AnsibleAssets releases the lock that keeps the \
         tree from being swept mid-play (#628)"
    );
}
