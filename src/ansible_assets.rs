use crate::output;
use eyre::{Context, Result};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const VERSION_STAMP: &str = ".auberge-version";
const COLLECTIONS_CACHE: &str = ".ansible";
const REQUIREMENTS: &str = "requirements.yml";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

static EMBEDDED_ANSIBLE: Dir = include_dir!("$CARGO_MANIFEST_DIR/ansible");

pub struct AnsibleAssets {
    ansible_dir: PathBuf,
}

impl AnsibleAssets {
    pub fn prepare() -> Result<Self> {
        Self::prepare_impl(std::env::var("AUBERGE_DEV").is_ok())
    }

    fn prepare_impl(dev_mode: bool) -> Result<Self> {
        if dev_mode {
            let dev_dir = PathBuf::from("ansible");
            if dev_dir.join("playbooks").exists() && dev_dir.join("roles").exists() {
                return Ok(Self {
                    ansible_dir: dev_dir,
                });
            }
        }

        if cfg!(test) {
            let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible");
            return Ok(Self {
                ansible_dir: dev_dir,
            });
        }

        let ansible_dir = crate::config::Config::data_dir()?.join("ansible");
        let fingerprint = embedded_fingerprint();

        if ensure_extracted(&ansible_dir, &fingerprint)? {
            eprintln!("Extracted ansible assets for v{}", fingerprint);
        }

        Ok(Self { ansible_dir })
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
        let collections_dir = self.ansible_dir.join(COLLECTIONS_CACHE).join("collections");
        if collections_dir.join("ansible_collections").exists() {
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
                .arg(&collections_dir),
        )
        .wrap_err("Failed to run ansible-galaxy. Is ansible installed?")?;
        if result.status.success() {
            output::clear_subprocess_lines(result.lines_written);
        }

        if !result.status.success() {
            eyre::bail!(
                "ansible-galaxy collection install failed with exit code {}",
                result.status.code().unwrap_or(-1)
            );
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

fn ensure_extracted(ansible_dir: &Path, fingerprint: &str) -> Result<bool> {
    let stamp = ansible_dir.join(VERSION_STAMP);
    if std::fs::read_to_string(&stamp).is_ok_and(|stamped| stamped.trim() == fingerprint) {
        return Ok(false);
    }

    if ansible_dir.exists() {
        clear_extracted(ansible_dir, collections_are_current(ansible_dir))?;
    }
    std::fs::create_dir_all(ansible_dir).wrap_err("Failed to create ansible dir")?;
    extract_dir(&EMBEDDED_ANSIBLE, ansible_dir)?;
    write_ansible_cfg(ansible_dir)?;
    std::fs::write(&stamp, fingerprint).wrap_err("Failed to write version stamp")?;
    Ok(true)
}

fn collections_are_current(ansible_dir: &Path) -> bool {
    let embedded = EMBEDDED_ANSIBLE
        .get_file(REQUIREMENTS)
        .map(|f| f.contents());
    let extracted = std::fs::read(ansible_dir.join(REQUIREMENTS)).ok();
    embedded == extracted.as_deref()
}

fn clear_extracted(ansible_dir: &Path, keep_collections: bool) -> Result<()> {
    for entry in std::fs::read_dir(ansible_dir).wrap_err("Failed to read stale ansible dir")? {
        let entry = entry.wrap_err("Failed to read stale ansible dir entry")?;
        if keep_collections && entry.file_name() == COLLECTIONS_CACHE {
            continue;
        }
        let path = entry.path();
        let removed = if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.wrap_err_with(|| format!("Failed to remove stale asset: {}", path.display()))?;
    }
    Ok(())
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

fn write_ansible_cfg(ansible_dir: &Path) -> Result<()> {
    let cfg = format!(
        "[defaults]\n\
         inventory = inventory.yml\n\
         roles_path = {roles}\n\
         remote_tmp = /tmp\n\
         collections_path = {collections}\n",
        roles = ansible_dir.join("roles").display(),
        collections = ansible_dir
            .join(COLLECTIONS_CACHE)
            .join("collections")
            .display(),
    );
    std::fs::write(ansible_dir.join("ansible.cfg"), cfg).wrap_err("Failed to write ansible.cfg")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // Gokapi rejects any custom favicon that is not exactly 512x512 by calling
    // os.Exit(1) during startup rather than falling back to its default, so a
    // wrong-sized asset here takes the service down on deploy instead of
    // degrading. Dimensions come straight from the PNG IHDR chunk: 8-byte
    // signature, 4-byte length, 4-byte type, then width and height as
    // big-endian u32.
    #[test]
    fn test_gokapi_custom_favicon_is_512_square() {
        let favicon = EMBEDDED_ANSIBLE
            .get_file("roles/gokapi/files/custom/favicon.png")
            .expect("gokapi role must ship custom/favicon.png");
        let bytes = favicon.contents();

        assert_eq!(
            &bytes[..8],
            b"\x89PNG\r\n\x1a\n",
            "custom/favicon.png must be a PNG — Gokapi decodes it as one"
        );
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!(
            (width, height),
            (512, 512),
            "Gokapi exits 1 at startup unless custom/favicon.png is exactly 512x512"
        );
    }

    #[test]
    fn test_ensure_extracted_writes_assets_and_stamp() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ansible");

        assert!(ensure_extracted(&dir, &fingerprint(1)).unwrap());

        assert!(dir.join("playbooks/apps.yml").is_file());
        assert!(dir.join("ansible.cfg").is_file());
        assert_eq!(
            std::fs::read_to_string(dir.join(VERSION_STAMP)).unwrap(),
            fingerprint(1)
        );
    }

    #[test]
    fn test_ensure_extracted_skips_when_fingerprint_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ansible");
        ensure_extracted(&dir, &fingerprint(1)).unwrap();
        std::fs::write(dir.join("sentinel"), "kept").unwrap();

        assert!(!ensure_extracted(&dir, &fingerprint(1)).unwrap());

        assert!(dir.join("sentinel").is_file());
    }

    #[test]
    fn test_changed_tree_at_same_version_triggers_reextract() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ansible");
        ensure_extracted(&dir, &fingerprint(1)).unwrap();
        std::fs::write(dir.join("sentinel"), "wiped").unwrap();

        assert!(ensure_extracted(&dir, &fingerprint(2)).unwrap());

        assert!(!dir.join("sentinel").exists());
        assert!(dir.join("playbooks/apps.yml").is_file());
        assert_eq!(
            std::fs::read_to_string(dir.join(VERSION_STAMP)).unwrap(),
            fingerprint(2)
        );
    }

    #[test]
    fn test_reextract_keeps_collections_when_requirements_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ansible");
        ensure_extracted(&dir, &fingerprint(1)).unwrap();
        let installed = dir
            .join(COLLECTIONS_CACHE)
            .join("collections/ansible_collections");
        std::fs::create_dir_all(&installed).unwrap();

        ensure_extracted(&dir, &fingerprint(2)).unwrap();

        assert!(installed.is_dir());
        assert!(dir.join("playbooks/apps.yml").is_file());
    }

    #[test]
    fn test_reextract_drops_collections_when_requirements_change() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ansible");
        ensure_extracted(&dir, &fingerprint(1)).unwrap();
        let installed = dir
            .join(COLLECTIONS_CACHE)
            .join("collections/ansible_collections");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(dir.join(REQUIREMENTS), "collections: []").unwrap();

        ensure_extracted(&dir, &fingerprint(2)).unwrap();

        assert!(!installed.exists());
    }

    #[test]
    fn test_fingerprints_share_version_but_differ_on_content() {
        assert!(fingerprint(1).starts_with(VERSION));
        assert!(fingerprint(2).starts_with(VERSION));
        assert_ne!(fingerprint(1), fingerprint(2));
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
