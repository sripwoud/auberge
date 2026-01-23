use eyre::{Context, Result};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};

static EMBEDDED_PLAYBOOKS: Dir = include_dir!("$CARGO_MANIFEST_DIR/ansible/playbooks");

pub struct PlaybookManager;

impl PlaybookManager {
    pub fn get_playbooks_dir() -> Result<PathBuf> {
        let dev_playbooks = PathBuf::from("ansible/playbooks");
        if dev_playbooks.exists() && dev_playbooks.is_dir() {
            return Ok(dev_playbooks);
        }

        let xdg_playbooks = crate::config::Config::data_dir()?.join("playbooks");
        if !xdg_playbooks.exists() {
            std::fs::create_dir_all(&xdg_playbooks)
                .wrap_err("Failed to create playbooks directory")?;
            Self::extract_embedded_playbooks(&xdg_playbooks)?;
        }

        Ok(xdg_playbooks)
    }

    fn extract_embedded_playbooks(target_dir: &Path) -> Result<()> {
        for entry in EMBEDDED_PLAYBOOKS.entries() {
            Self::extract_entry(entry, target_dir)?;
        }
        Ok(())
    }

    fn extract_entry(entry: &include_dir::DirEntry, base_path: &Path) -> Result<()> {
        match entry {
            include_dir::DirEntry::Dir(dir) => {
                let dir_path = base_path.join(dir.path());
                std::fs::create_dir_all(&dir_path).wrap_err_with(|| {
                    format!("Failed to create directory: {}", dir_path.display())
                })?;

                for sub_entry in dir.entries() {
                    Self::extract_entry(sub_entry, base_path)?;
                }
            }
            include_dir::DirEntry::File(file) => {
                let file_path = base_path.join(file.path());
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent).wrap_err_with(|| {
                        format!("Failed to create parent directory: {}", parent.display())
                    })?;
                }
                std::fs::write(&file_path, file.contents())
                    .wrap_err_with(|| format!("Failed to write file: {}", file_path.display()))?;
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_available_playbooks() -> Result<Vec<String>> {
        let playbooks_dir = Self::get_playbooks_dir()?;
        let mut playbooks = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&playbooks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("yml")
                    && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                {
                    playbooks.push(name.to_string());
                }
            }
        }

        playbooks.sort();
        Ok(playbooks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_available_playbooks() {
        let result = PlaybookManager::list_available_playbooks();
        assert!(result.is_ok());
    }
}
