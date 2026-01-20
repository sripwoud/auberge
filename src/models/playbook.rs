use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Playbook {
    pub path: PathBuf,
    pub name: String,
}

impl Playbook {
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        Self { path, name }
    }
}
