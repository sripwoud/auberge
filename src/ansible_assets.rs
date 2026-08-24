use crate::output;
use eyre::{Context, Result};
use include_dir::{Dir, include_dir};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LOCK: &str = ".lock";
const COLLECTIONS: &str = "collections";
const INSTALLED_COLLECTIONS: &str = "ansible_collections";
const REQUIREMENTS: &str = "requirements.yml";
const STAGING_PREFIX: &str = ".staging";
const WORKING_TREE_CACHE: &str = ".ansible";
const LEGACY_CACHE: &str = ".ansible";
const LEGACY_STAMP: &str = ".auberge-version";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

static EMBEDDED_ANSIBLE: Dir = include_dir!("$CARGO_MANIFEST_DIR/ansible");

pub struct AnsibleAssets {
    ansible_dir: PathBuf,
    collections_dir: PathBuf,
    _tree_lock: Option<File>,
}

impl AnsibleAssets {
    pub fn prepare() -> Result<Self> {
        Self::prepare_impl(std::env::var("AUBERGE_DEV").is_ok())
    }

    fn prepare_impl(dev_mode: bool) -> Result<Self> {
        if dev_mode {
            let dev_dir = PathBuf::from("ansible");
            if dev_dir.join("playbooks").exists() && dev_dir.join("roles").exists() {
                return Ok(Self::from_working_tree(dev_dir));
            }
        }

        if cfg!(test) {
            return Ok(Self::from_working_tree(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible"),
            ));
        }

        open_tree(
            &crate::config::Config::data_dir()?.join("ansible"),
            &embedded_fingerprint(),
        )
    }

    fn from_working_tree(ansible_dir: PathBuf) -> Self {
        let collections_dir = ansible_dir.join(WORKING_TREE_CACHE).join(COLLECTIONS);
        Self {
            ansible_dir,
            collections_dir,
            _tree_lock: None,
        }
    }

    pub fn ansible_dir(&self) -> &Path {
        &self.ansible_dir
    }

    pub fn playbooks_dir(&self) -> PathBuf {
        self.ansible_dir.join("playbooks")
    }

    pub fn roles_dir(&self) -> PathBuf {
        self.ansible_dir.join("roles")
    }

    pub fn ensure_collections(&self) -> Result<()> {
        if self.collections_dir.join(INSTALLED_COLLECTIONS).exists() {
            return Ok(());
        }

        let requirements = self.ansible_dir.join(REQUIREMENTS);
        if !requirements.exists() {
            return Ok(());
        }

        eprintln!("Installing ansible collections (one-time)...");
        let result = output::run_piped(
            "ansible-galaxy",
            std::process::Command::new("ansible-galaxy")
                .arg("collection")
                .arg("install")
                .arg("-r")
                .arg(&requirements)
                .arg("-p")
                .arg(&self.collections_dir),
        )
        .wrap_err("Failed to run ansible-galaxy. Is ansible installed?")?;
        if result.status.success() {
            output::clear_subprocess_lines(result.lines_written);
        }

        if !result.status.success() {
            return Err(result.error(format!(
                "ansible-galaxy collection install failed with exit code {}",
                result.status.code().unwrap_or(-1)
            )));
        }

        Ok(())
    }
}

fn embedded_fingerprint() -> String {
    fingerprint(hash_dir(&EMBEDDED_ANSIBLE))
}

fn fingerprint(content_hash: u64) -> String {
    format!("{}+{:016x}", VERSION, content_hash)
}

fn requirements_fingerprint() -> String {
    let contents = EMBEDDED_ANSIBLE
        .get_file(REQUIREMENTS)
        .map(|f| f.contents())
        .unwrap_or_default();
    format!("{:016x}", hash_chunk(FNV_OFFSET, contents))
}

fn hash_dir(dir: &Dir<'static>) -> u64 {
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    files.sort_unstable_by_key(|(path, _)| *path);
    hash_files(&files)
}

fn collect_files(dir: &Dir<'static>, files: &mut Vec<(&'static Path, &'static [u8])>) {
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(sub) => collect_files(sub, files),
            include_dir::DirEntry::File(file) => files.push((file.path(), file.contents())),
        }
    }
}

fn hash_files(files: &[(&Path, &[u8])]) -> u64 {
    let mut hash = FNV_OFFSET;
    for (path, contents) in files {
        hash = hash_chunk(hash, path.as_os_str().as_encoded_bytes());
        hash = hash_chunk(hash, contents);
    }
    hash
}

fn hash_chunk(hash: u64, bytes: &[u8]) -> u64 {
    hash_bytes(hash_bytes(hash, &(bytes.len() as u64).to_le_bytes()), bytes)
}

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn open_tree(root: &Path, fingerprint: &str) -> Result<AnsibleAssets> {
    std::fs::create_dir_all(root).wrap_err("Failed to create ansible assets dir")?;
    let root_lock = open_lock(&root.join(LOCK))?;
    root_lock
        .lock()
        .wrap_err("Failed to lock ansible assets dir")?;

    let ansible_dir = root.join(fingerprint);
    let collections_dir = root.join(COLLECTIONS).join(requirements_fingerprint());

    if !ansible_dir.exists() {
        extract_tree(root, &ansible_dir, &collections_dir)?;
        eprintln!("Extracted ansible assets for v{}", fingerprint);
    }

    let tree_lock = open_lock(&ansible_dir.join(LOCK))?;
    tree_lock
        .lock_shared()
        .wrap_err_with(|| format!("Failed to lock ansible assets: {}", ansible_dir.display()))?;

    sweep_unused(root, fingerprint);

    Ok(AnsibleAssets {
        ansible_dir,
        collections_dir,
        _tree_lock: Some(tree_lock),
    })
}

fn open_lock(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .wrap_err_with(|| format!("Failed to open lock file: {}", path.display()))
}

fn extract_tree(root: &Path, ansible_dir: &Path, collections_dir: &Path) -> Result<()> {
    let staging = tempfile::Builder::new()
        .prefix(STAGING_PREFIX)
        .tempdir_in(root)
        .wrap_err("Failed to stage ansible assets")?;
    extract_dir(&EMBEDDED_ANSIBLE, staging.path())?;
    write_ansible_cfg(staging.path(), ansible_dir, collections_dir)?;
    File::create(staging.path().join(LOCK)).wrap_err("Failed to write asset lock file")?;

    let staged = staging.keep();
    std::fs::rename(&staged, ansible_dir).wrap_err_with(|| {
        format!(
            "Failed to publish ansible assets: {}",
            ansible_dir.display()
        )
    })
}

fn sweep_unused(root: &Path, fingerprint: &str) {
    if let Err(err) = collect_unused(root, fingerprint) {
        output::warn(&format!("Left unused ansible assets in place: {:#}", err));
    }
}

fn collect_unused(root: &Path, fingerprint: &str) -> Result<()> {
    let listing = std::fs::read_dir(root)
        .wrap_err("Failed to read ansible assets dir")?
        .collect::<std::io::Result<Vec<_>>>()
        .wrap_err("Failed to read ansible assets dir entry")?;

    for entry in listing {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == fingerprint || name == LOCK || name == COLLECTIONS {
            continue;
        }

        let kind = entry
            .file_type()
            .wrap_err_with(|| format!("Failed to inspect: {}", entry.path().display()))?;
        let path = entry.path();

        if is_tree(&name) && kind.is_dir() {
            if !tree_is_idle(&path)? {
                continue;
            }
        } else if !(is_staging(&name) || is_legacy(&name)) {
            continue;
        }

        discard(root, &path)?;
    }
    Ok(())
}

fn is_tree(name: &str) -> bool {
    match name.rsplit_once('+') {
        Some((version, hash)) => {
            !version.is_empty() && hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit())
        }
        None => false,
    }
}

fn is_staging(name: &str) -> bool {
    name.starts_with(STAGING_PREFIX)
}

fn is_legacy(name: &str) -> bool {
    name == LEGACY_STAMP
        || name == LEGACY_CACHE
        || EMBEDDED_ANSIBLE.get_dir(name).is_some()
        || EMBEDDED_ANSIBLE.get_file(name).is_some()
}

fn tree_is_idle(tree: &Path) -> Result<bool> {
    let path = tree.join(LOCK);
    let lock = match File::open(&path) {
        Ok(lock) => lock,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => {
            return Err(err)
                .wrap_err_with(|| format!("Failed to open lock file: {}", path.display()));
        }
    };

    match lock.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(err)) => {
            Err(err).wrap_err_with(|| format!("Failed to test lock file: {}", path.display()))
        }
    }
}

fn discard(root: &Path, path: &Path) -> Result<()> {
    let entry = std::fs::symlink_metadata(path)
        .wrap_err_with(|| format!("Failed to inspect: {}", path.display()))?;
    if !entry.is_dir() {
        return std::fs::remove_file(path)
            .wrap_err_with(|| format!("Failed to remove: {}", path.display()));
    }

    let grave = tempfile::Builder::new()
        .prefix(STAGING_PREFIX)
        .tempdir_in(root)
        .wrap_err("Failed to stage unused ansible assets for removal")?;
    let doomed = grave.path().join("assets");
    std::fs::rename(path, &doomed)
        .wrap_err_with(|| format!("Failed to retire: {}", path.display()))?;
    std::fs::remove_dir_all(&doomed)
        .wrap_err_with(|| format!("Failed to remove: {}", path.display()))
}

fn extract_dir(dir: &Dir, base: &Path) -> Result<()> {
    for entry in dir.entries() {
        extract_entry(entry, base)?;
    }
    Ok(())
}

fn extract_entry(entry: &include_dir::DirEntry, base: &Path) -> Result<()> {
    match entry {
        include_dir::DirEntry::Dir(dir) => {
            let dest = base.join(dir.path());
            std::fs::create_dir_all(&dest)
                .wrap_err_with(|| format!("Failed to create dir: {}", dest.display()))?;
            for sub in dir.entries() {
                extract_entry(sub, base)?;
            }
        }
        include_dir::DirEntry::File(file) => {
            let dest = base.join(file.path());
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, file.contents())
                .wrap_err_with(|| format!("Failed to write: {}", dest.display()))?;
        }
    }
    Ok(())
}

fn write_ansible_cfg(staging: &Path, published: &Path, collections_dir: &Path) -> Result<()> {
    let cfg = format!(
        "[defaults]\n\
         inventory = inventory.yml\n\
         roles_path = {roles}\n\
         remote_tmp = /tmp\n\
         collections_path = {collections}\n",
        roles = published.join("roles").display(),
        collections = collections_dir.display(),
    );
    std::fs::write(staging.join("ansible.cfg"), cfg).wrap_err("Failed to write ansible.cfg")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_a() -> String {
        fingerprint(1)
    }

    fn tree_b() -> String {
        fingerprint(2)
    }

    fn staging_dirs(root: &Path) -> Vec<String> {
        std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| is_staging(name))
            .collect()
    }

    #[test]
    fn test_prepare_impl_uses_dev_mode() {
        let assets = AnsibleAssets::prepare_impl(true).unwrap();
        assert_eq!(assets.ansible_dir(), Path::new("ansible"));
    }

    #[test]
    fn test_embedded_ansible_has_roles() {
        assert!(EMBEDDED_ANSIBLE.get_dir("roles").is_some());
    }

    #[test]
    fn test_embedded_ansible_has_playbooks() {
        assert!(EMBEDDED_ANSIBLE.get_dir("playbooks").is_some());
    }

    #[test]
    fn test_extract_dir_writes_files_to_correct_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("ansible");
        std::fs::create_dir_all(&base).unwrap();
        extract_dir(&EMBEDDED_ANSIBLE, &base).unwrap();

        assert!(base.join("playbooks").is_dir());
        assert!(base.join("roles").is_dir());
        assert!(base.join("requirements.yml").is_file());
        assert!(base.join("playbooks/apps.yml").is_file());
    }

    // Branding assets are operator-supplied and deliberately absent from the
    // embedded tree, so no role ships another operator's theme or icon. The
    // 512x512 favicon requirement Gokapi enforces with os.Exit(1) is checked at
    // deploy time against the operator's own file instead — see the gokapi
    // role's "Require the operator favicon to be exactly 512x512" task.
    #[test]
    fn test_no_role_ships_branding_assets() {
        assert!(
            EMBEDDED_ANSIBLE
                .get_dir("roles/gokapi/files/custom")
                .is_none(),
            "gokapi must not ship branding assets: they would become every operator's default"
        );
    }

    #[test]
    fn test_open_tree_extracts_under_the_fingerprint() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let assets = open_tree(root, &tree_a()).unwrap();

        assert_eq!(assets.ansible_dir(), root.join(tree_a()));
        assert!(assets.ansible_dir().join("playbooks/apps.yml").is_file());
        assert!(assets.ansible_dir().join("ansible.cfg").is_file());
        assert!(assets.ansible_dir().join(LOCK).is_file());
    }

    #[test]
    fn test_open_tree_leaves_no_staging_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let _assets = open_tree(root, &tree_a()).unwrap();

        assert!(staging_dirs(root).is_empty());
    }

    #[test]
    fn test_reopening_the_same_fingerprint_reuses_the_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let first = open_tree(root, &tree_a()).unwrap();
        std::fs::write(first.ansible_dir().join("sentinel"), "kept").unwrap();

        let second = open_tree(root, &tree_a()).unwrap();

        assert_eq!(second.ansible_dir(), first.ansible_dir());
        assert!(second.ansible_dir().join("sentinel").is_file());
    }

    #[test]
    fn test_a_second_fingerprint_gets_its_own_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let held = open_tree(root, &tree_a()).unwrap();
        std::fs::write(held.ansible_dir().join("sentinel"), "in use").unwrap();

        let other = open_tree(root, &tree_b()).unwrap();

        assert_ne!(held.ansible_dir(), other.ansible_dir());
        assert!(held.ansible_dir().join("sentinel").is_file());
        assert!(held.ansible_dir().join("playbooks/apps.yml").is_file());
        assert!(other.ansible_dir().join("playbooks/apps.yml").is_file());
    }

    #[test]
    fn test_sweep_keeps_a_tree_another_process_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let elsewhere = root.join(tree_b());
        std::fs::create_dir_all(&elsewhere).unwrap();
        let held = open_lock(&elsewhere.join(LOCK)).unwrap();
        held.lock_shared().unwrap();

        open_tree(root, &tree_a()).unwrap();

        assert!(elsewhere.is_dir());
    }

    #[test]
    fn test_sweep_removes_a_tree_nobody_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let abandoned = open_tree(root, &tree_b())
            .unwrap()
            .ansible_dir()
            .to_path_buf();

        open_tree(root, &tree_a()).unwrap();

        assert!(!abandoned.exists());
    }

    #[test]
    fn test_sweep_removes_the_flat_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("playbooks")).unwrap();
        std::fs::create_dir_all(root.join(WORKING_TREE_CACHE).join(COLLECTIONS)).unwrap();
        std::fs::write(root.join(LEGACY_STAMP), fingerprint(9)).unwrap();
        std::fs::write(root.join("ansible.cfg"), "[defaults]").unwrap();

        open_tree(root, &tree_a()).unwrap();

        assert!(!root.join("playbooks").exists());
        assert!(!root.join(WORKING_TREE_CACHE).exists());
        assert!(!root.join(LEGACY_STAMP).exists());
        assert!(!root.join("ansible.cfg").exists());
    }

    #[test]
    fn test_sweep_removes_abandoned_staging_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let abandoned = root.join(format!("{}abandoned", STAGING_PREFIX));
        std::fs::create_dir_all(&abandoned).unwrap();

        open_tree(root, &tree_a()).unwrap();

        assert!(!abandoned.exists());
    }

    #[test]
    fn test_sweep_keeps_an_entry_that_only_looks_like_a_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let named_dir = root.join("notes+draft");
        let named_file = root.join(tree_b());
        std::fs::create_dir_all(&named_dir).unwrap();
        std::fs::write(&named_file, "not a tree").unwrap();

        open_tree(root, &tree_a()).unwrap();

        assert!(named_dir.is_dir());
        assert!(named_file.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn test_a_sweep_that_cannot_finish_neither_fails_nor_leaves_the_name() {
        use std::os::unix::fs::PermissionsExt;

        fn set_mode(path: &Path, mode: u32) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let undeletable = root.join(tree_b());
        std::fs::create_dir_all(undeletable.join("sub")).unwrap();
        std::fs::write(undeletable.join("sub/file"), "pinned").unwrap();
        File::create(undeletable.join(LOCK)).unwrap();
        set_mode(&undeletable.join("sub"), 0o500);

        let assets = open_tree(root, &tree_a()).unwrap();

        assert!(assets.ansible_dir().join("playbooks/apps.yml").is_file());
        assert!(!undeletable.exists());

        for name in staging_dirs(root) {
            set_mode(&root.join(name).join("assets/sub"), 0o700);
        }
    }

    #[test]
    fn test_sweep_keeps_what_it_does_not_recognise() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("operator-notes.md"), "mine").unwrap();

        open_tree(root, &tree_a()).unwrap();

        assert!(root.join("operator-notes.md").is_file());
    }

    #[test]
    fn test_collections_are_shared_across_fingerprints() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let first = open_tree(root, &tree_a()).unwrap();
        let second = open_tree(root, &tree_b()).unwrap();

        assert_eq!(first.collections_dir, second.collections_dir);
        assert_eq!(
            first.collections_dir,
            root.join(COLLECTIONS).join(requirements_fingerprint())
        );
    }

    #[test]
    fn test_ansible_cfg_resolves_roles_and_collections_absolutely() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let assets = open_tree(root, &tree_a()).unwrap();

        let cfg = std::fs::read_to_string(assets.ansible_dir().join("ansible.cfg")).unwrap();

        assert!(cfg.contains(&format!(
            "roles_path = {}\n",
            assets.ansible_dir().join("roles").display()
        )));
        assert!(cfg.contains(&format!(
            "collections_path = {}\n",
            assets.collections_dir.display()
        )));
    }

    #[test]
    fn test_fingerprints_share_version_but_differ_on_content() {
        assert!(fingerprint(1).starts_with(VERSION));
        assert!(fingerprint(2).starts_with(VERSION));
        assert_ne!(fingerprint(1), fingerprint(2));
    }

    #[test]
    fn test_only_a_fingerprint_is_recognisable_as_a_tree() {
        assert!(is_tree(&embedded_fingerprint()));
        assert!(is_tree(&fingerprint(1)));
        assert!(!is_tree("playbooks"));
        assert!(!is_tree(COLLECTIONS));
        assert!(!is_tree("notes+draft"));
        assert!(!is_tree("+c372d5766de336fa"));
        assert!(!is_tree("0.15.15+c372d5766de336f"));
        assert!(!is_tree("0.15.15+zzzzzzzzzzzzzzzz"));
    }

    #[test]
    fn test_hash_files_detects_content_change() {
        let before = hash_files(&[(Path::new("roles/bichon/tasks/main.yml"), b"- name: a")]);
        let after = hash_files(&[(Path::new("roles/bichon/tasks/main.yml"), b"- name: b")]);
        assert_ne!(before, after);
    }

    #[test]
    fn test_hash_files_detects_renamed_file() {
        let before = hash_files(&[(Path::new("playbooks/apps.yml"), b"same")]);
        let after = hash_files(&[(Path::new("playbooks/infra.yml"), b"same")]);
        assert_ne!(before, after);
    }

    #[test]
    fn test_hash_files_is_unambiguous_across_boundaries() {
        let split = hash_files(&[(Path::new("a"), b"bc"), (Path::new("d"), b"e")]);
        let shifted = hash_files(&[(Path::new("ab"), b"c"), (Path::new("de"), b"")]);
        assert_ne!(split, shifted);
    }

    #[test]
    fn test_hash_dir_is_deterministic() {
        assert_eq!(hash_dir(&EMBEDDED_ANSIBLE), hash_dir(&EMBEDDED_ANSIBLE));
    }

    #[test]
    fn test_embedded_paths_have_no_ansible_prefix() {
        for entry in EMBEDDED_ANSIBLE.entries() {
            let path = match entry {
                include_dir::DirEntry::Dir(d) => d.path().to_path_buf(),
                include_dir::DirEntry::File(f) => f.path().to_path_buf(),
            };
            assert!(
                !path.starts_with("ansible"),
                "Entry path {path:?} should NOT start with 'ansible/' — include_dir paths are relative to the included directory"
            );
        }
    }
}
