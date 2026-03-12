use eyre::{Context, Result};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const VERSION_STAMP: &str = ".auberge-version";

static EMBEDDED_ANSIBLE: Dir = include_dir!("$CARGO_MANIFEST_DIR/ansible");

pub struct AnsibleAssets {
    ansible_dir: PathBuf,
}

impl AnsibleAssets {
    pub fn prepare() -> Result<Self> {
        if cfg!(test) {
            let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible");
            return Ok(Self {
                ansible_dir: dev_dir,
            });
        }

        if std::env::var("AUBERGE_DEV").is_ok() {
            let dev_dir = PathBuf::from("ansible");
            if dev_dir.join("playbooks").exists() && dev_dir.join("roles").exists() {
                return Ok(Self {
                    ansible_dir: dev_dir,
                });
            }
        }

        let ansible_dir = crate::config::Config::data_dir()?.join("ansible");
        let stamp = ansible_dir.join(VERSION_STAMP);

        let needs_extract = !stamp.exists()
            || std::fs::read_to_string(&stamp)
                .map(|v| v.trim() != VERSION)
                .unwrap_or(true);

        if needs_extract {
            if ansible_dir.exists() {
                std::fs::remove_dir_all(&ansible_dir)
                    .wrap_err("Failed to remove stale ansible dir")?;
            }
            std::fs::create_dir_all(&ansible_dir).wrap_err("Failed to create ansible dir")?;
            extract_dir(&EMBEDDED_ANSIBLE, &ansible_dir)?;
            write_ansible_cfg(&ansible_dir)?;
            std::fs::write(&stamp, VERSION).wrap_err("Failed to write version stamp")?;
            eprintln!("Extracted ansible assets for v{}", VERSION);
        }

        Ok(Self { ansible_dir })
    }

    pub fn ansible_dir(&self) -> &Path {
        &self.ansible_dir
    }

    pub fn playbooks_dir(&self) -> PathBuf {
        self.ansible_dir.join("playbooks")
    }

    pub fn ensure_collections(&self) -> Result<()> {
        let collections_dir = self.ansible_dir.join(".ansible/collections");
        if collections_dir.exists() {
            return Ok(());
        }

        let requirements = self.ansible_dir.join("requirements.yml");
        if !requirements.exists() {
            return Ok(());
        }

        eprintln!("Installing ansible collections (one-time)...");
        let status = std::process::Command::new("ansible-galaxy")
            .arg("collection")
            .arg("install")
            .arg("-r")
            .arg(&requirements)
            .arg("-p")
            .arg(&collections_dir)
            .status()
            .wrap_err("Failed to run ansible-galaxy. Is ansible installed?")?;

        if !status.success() {
            eyre::bail!(
                "ansible-galaxy collection install failed with exit code {}",
                status.code().unwrap_or(-1)
            );
        }

        Ok(())
    }
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
            let rel = strip_top_component(dir.path());
            let dest = base.join(rel);
            std::fs::create_dir_all(&dest)
                .wrap_err_with(|| format!("Failed to create dir: {}", dest.display()))?;
            for sub in dir.entries() {
                extract_entry(sub, base)?;
            }
        }
        include_dir::DirEntry::File(file) => {
            let rel = strip_top_component(file.path());
            let dest = base.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, file.contents())
                .wrap_err_with(|| format!("Failed to write: {}", dest.display()))?;
        }
    }
    Ok(())
}

fn strip_top_component(p: &Path) -> &Path {
    let mut components = p.components();
    components.next();
    components.as_path()
}

fn write_ansible_cfg(ansible_dir: &Path) -> Result<()> {
    let cfg = format!(
        "[defaults]\n\
         inventory = inventory.yml\n\
         roles_path = {roles}\n\
         remote_tmp = /tmp\n\
         collections_path = {collections}\n",
        roles = ansible_dir.join("roles").display(),
        collections = ansible_dir.join(".ansible/collections").display(),
    );
    std::fs::write(ansible_dir.join("ansible.cfg"), cfg).wrap_err("Failed to write ansible.cfg")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_top_component() {
        assert_eq!(
            strip_top_component(Path::new("ansible/roles/caddy")),
            Path::new("roles/caddy")
        );
        assert_eq!(
            strip_top_component(Path::new("ansible/playbooks/apps.yml")),
            Path::new("playbooks/apps.yml")
        );
    }

    #[test]
    fn test_prepare_uses_dev_mode() {
        unsafe {
            std::env::set_var("AUBERGE_DEV", "1");
        }
        let assets = AnsibleAssets::prepare().unwrap();
        assert!(assets.playbooks_dir().ends_with("playbooks"));
        unsafe {
            std::env::remove_var("AUBERGE_DEV");
        }
    }

    #[test]
    fn test_embedded_ansible_has_roles() {
        assert!(EMBEDDED_ANSIBLE.get_dir("roles").is_some());
    }

    #[test]
    fn test_embedded_ansible_has_playbooks() {
        assert!(EMBEDDED_ANSIBLE.get_dir("playbooks").is_some());
    }
}
