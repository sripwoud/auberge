use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fail2ban_role_dir() -> PathBuf {
    repo_root().join("ansible/roles/fail2ban")
}

/// Render jail.local.j2 the way ansible will: ansible.builtin.template
/// defaults trim_blocks to true (jinja2's native default is false), which
/// eats the newline after a trailing block tag and can join the next
/// directive onto the ignoreip line (#582 review).
fn render_jail_local(hosts: &str, extra: &str) -> String {
    let template = fs::read_to_string(fail2ban_role_dir().join("templates/jail.local.j2"))
        .expect("jail.local.j2 must exist");

    let mut ctx: HashMap<&str, String> = HashMap::new();
    for key in [
        "fail2ban_bantime",
        "fail2ban_findtime",
        "fail2ban_maxretry",
        "ssh_port",
        "fail2ban_ssh_maxretry",
        "fail2ban_ssh_bantime",
        "fail2ban_sshddos_maxretry",
        "fail2ban_sshddos_bantime",
        "fail2ban_recidive_bantime",
        "fail2ban_recidive_maxretry",
    ] {
        ctx.insert(key, "1".to_string());
    }
    ctx.insert("fail2ban_ignoreip_hosts", hosts.to_string());
    ctx.insert("fail2ban_ignoreip_extra", extra.to_string());

    let mut env = minijinja::Environment::new();
    env.set_trim_blocks(true);
    env.render_str(&template, ctx)
        .expect("jail.local.j2 must render")
}

fn ignoreip_line(rendered: &str) -> String {
    rendered
        .lines()
        .find(|line| line.trim_start().starts_with("ignoreip"))
        .expect("rendered jail.local must set ignoreip")
        .to_string()
}

#[test]
fn test_ignoreip_covers_loopback_and_tailnet_by_default() {
    let line = ignoreip_line(&render_jail_local("", ""));
    assert!(line.contains("127.0.0.1/8"));
    assert!(line.contains("::1"));
    assert!(line.ends_with("100.64.0.0/10"));
}

#[test]
fn test_ignoreip_sits_in_the_default_section() {
    let rendered = render_jail_local("", "");
    let ignoreip_pos = rendered.find("ignoreip").expect("ignoreip present");
    let first_jail_pos = rendered.find("[sshd]").expect("sshd jail present");
    assert!(ignoreip_pos < first_jail_pos);
}

#[test]
fn test_host_ips_arrive_comma_separated_and_render_space_separated() {
    let line = ignoreip_line(&render_jail_local("203.0.113.7,198.51.100.9", ""));
    assert!(line.ends_with("203.0.113.7 198.51.100.9"));
    assert!(!line.contains(','));
}

#[test]
fn test_operator_extra_list_is_appended_verbatim() {
    let line = ignoreip_line(&render_jail_local("", "192.0.2.1 198.51.100.0/24"));
    assert!(line.ends_with("192.0.2.1 198.51.100.0/24"));
}

#[test]
fn test_directive_after_ignoreip_survives_ansible_trim_blocks() {
    for (hosts, extra) in [
        ("", ""),
        ("203.0.113.7", ""),
        ("", "192.0.2.1"),
        ("203.0.113.7", "192.0.2.1"),
    ] {
        let rendered = render_jail_local(hosts, extra);
        assert!(
            rendered
                .lines()
                .any(|line| line.trim() == "destemail = root@localhost"),
            "destemail must keep its own line (hosts={hosts:?}, extra={extra:?}):\n{rendered}"
        );
    }
}

#[test]
fn test_role_defaults_declare_both_ignoreip_vars_empty() {
    let defaults = fs::read_to_string(fail2ban_role_dir().join("defaults/main.yml"))
        .expect("fail2ban defaults must exist");
    let parsed: serde_yaml::Value = serde_yaml::from_str(&defaults).unwrap();
    assert_eq!(parsed["fail2ban_ignoreip_hosts"].as_str(), Some(""));
    assert_eq!(parsed["fail2ban_ignoreip_extra"].as_str(), Some(""));
}

#[test]
fn test_key_registry_offers_the_operator_ignoreip_key() {
    let keys = fs::read_to_string(repo_root().join("ansible/keys.yml")).expect("keys.yml");
    let parsed: serde_yaml::Value = serde_yaml::from_str(&keys).unwrap();
    let key = &parsed["keys"]["fail2ban_ignoreip_extra"];
    assert!(
        key.is_mapping(),
        "fail2ban_ignoreip_extra missing from keys.yml"
    );
    assert_eq!(key["secret"].as_bool(), Some(false));
}
