use std::fs;
use std::path::PathBuf;

fn roles_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles")
}

fn is_version_variable(line: &str) -> Option<&str> {
    let (key, _) = line.split_once(':')?;
    (key.ends_with("_version")
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
    .then_some(key)
}

#[test]
fn test_every_version_variable_carries_a_renovate_annotation() {
    let mut checked = 0;
    let mut violations = Vec::new();

    for entry in fs::read_dir(roles_dir()).expect("ansible/roles must exist") {
        let defaults = entry.unwrap().path().join("defaults/main.yml");
        if !defaults.exists() {
            continue;
        }
        let content = fs::read_to_string(&defaults).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let Some(key) = is_version_variable(line) else {
                continue;
            };
            checked += 1;
            let annotated = i > 0
                && lines[i - 1].starts_with("# renovate: datasource=")
                && lines[i - 1].contains(" depName=");
            if !annotated {
                violations.push(format!("{}: {key}", defaults.display()));
            }
        }
    }

    assert!(
        checked >= 16,
        "expected at least 16 version variables, found {checked} — did the roles layout move?"
    );
    assert!(
        violations.is_empty(),
        "version variables missing a `# renovate: datasource=… depName=…` annotation \
         (see renovate.json customManagers):\n{}",
        violations.join("\n")
    );
}
