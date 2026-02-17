use eyre::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct SshConfigHost {
    pub name: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

pub struct SshConfigParser {
    path: PathBuf,
}

impl SshConfigParser {
    pub fn new() -> Result<Self> {
        let expanded = shellexpand::tilde("~/.ssh/config");
        let path = PathBuf::from(expanded.as_ref());
        Ok(Self { path })
    }

    pub fn parse(&self) -> Result<Vec<SshConfigHost>> {
        if !self.path.exists() {
            return Ok(vec![]);
        }

        let content = std::fs::read_to_string(&self.path)?;
        let hosts = parse_content(&content);

        let filtered: Vec<SshConfigHost> = hosts
            .into_iter()
            .filter(|h| {
                if h.hostname.is_none() {
                    return false;
                }
                let has_wildcard =
                    h.name.contains('*') || h.name.contains('?') || h.name.contains('!');
                !has_wildcard
            })
            .collect();

        Ok(filtered)
    }
}

fn parse_content(content: &str) -> Vec<SshConfigHost> {
    let mut hosts = Vec::new();
    let mut current_host: Option<SshConfigHost> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let directive = parts[0].to_lowercase();

        match directive.as_str() {
            "host" => {
                if let Some(host) = current_host.take() {
                    hosts.push(host);
                }

                if parts.len() > 1 {
                    current_host = Some(SshConfigHost {
                        name: parts[1].to_string(),
                        hostname: None,
                        user: None,
                        port: None,
                        identity_file: None,
                    });
                }
            }
            "hostname" => {
                if let Some(ref mut host) = current_host
                    && parts.len() > 1
                {
                    host.hostname = Some(parts[1].to_string());
                }
            }
            "user" => {
                if let Some(ref mut host) = current_host
                    && parts.len() > 1
                {
                    host.user = Some(parts[1].to_string());
                }
            }
            "port" => {
                if let Some(ref mut host) = current_host
                    && parts.len() > 1
                    && let Ok(port_num) = parts[1].parse::<u16>()
                {
                    host.port = Some(port_num);
                }
            }
            "identityfile" => {
                if let Some(ref mut host) = current_host
                    && parts.len() > 1
                {
                    let expanded = shellexpand::tilde(parts[1]);
                    host.identity_file = Some(expanded.into_owned());
                }
            }
            _ => {}
        }
    }

    if let Some(host) = current_host {
        hosts.push(host);
    }

    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_host() {
        let content = r#"
Host myserver
    HostName 192.168.1.100
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_rsa
"#;
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 1);

        let host = &hosts[0];
        assert_eq!(host.name, "myserver");
        assert_eq!(host.hostname, Some("192.168.1.100".to_string()));
        assert_eq!(host.user, Some("admin".to_string()));
        assert_eq!(host.port, Some(2222));
        assert!(host.identity_file.is_some());
        assert!(host.identity_file.as_ref().unwrap().starts_with("/"));
    }

    #[test]
    fn test_parse_multiple_hosts() {
        let content = r#"
Host server1
    HostName 10.0.0.1
    User root

Host server2
    HostName 10.0.0.2
    User admin
    Port 22
"#;
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "server1");
        assert_eq!(hosts[1].name, "server2");
    }

    #[test]
    fn test_handle_comments_and_blank_lines() {
        let content = r#"
# This is a comment
Host myserver
    # Another comment
    HostName 192.168.1.100

    User admin
"#;
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, Some("192.168.1.100".to_string()));
    }

    #[test]
    fn test_filter_wildcard_hosts() {
        let content = r#"
Host *.example.com
    User admin

Host server1
    HostName 10.0.0.1

Host *
    User defaultuser
"#;
        let hosts = parse_content(content);

        let filtered: Vec<SshConfigHost> = hosts
            .into_iter()
            .filter(|h| {
                if h.hostname.is_none() {
                    return false;
                }
                let has_wildcard =
                    h.name.contains('*') || h.name.contains('?') || h.name.contains('!');
                !has_wildcard
            })
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "server1");
    }

    #[test]
    fn test_skip_hosts_without_hostname() {
        let content = r#"
Host alias-only
    User admin

Host real-host
    HostName 10.0.0.1
    User admin
"#;
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 2);

        let filtered: Vec<SshConfigHost> =
            hosts.into_iter().filter(|h| h.hostname.is_some()).collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "real-host");
    }

    #[test]
    fn test_expand_tilde_in_identity_file() {
        let content = r#"
Host myserver
    HostName 192.168.1.100
    IdentityFile ~/.ssh/custom_key
"#;
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 1);

        let identity = hosts[0].identity_file.as_ref().unwrap();
        assert!(identity.starts_with('/'));
        assert!(!identity.contains('~'));
    }

    #[test]
    fn test_handle_invalid_port() {
        let content = r#"
Host myserver
    HostName 192.168.1.100
    Port invalid
"#;
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].port, None);
    }

    #[test]
    fn test_empty_file_returns_empty_vec() {
        let content = "";
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 0);
    }

    #[test]
    fn test_only_comments() {
        let content = r#"
# Just comments
# No actual hosts
"#;
        let hosts = parse_content(content);
        assert_eq!(hosts.len(), 0);
    }

    #[test]
    fn test_realistic_ssh_config() {
        let content = r#"
AddKeysToAgent yes

Host github github.com
  HostName github.com
  IdentityFile ~/.ssh/identities/github
  IdentitiesOnly yes
  PreferredAuthentications publickey

Host staging
  HostName 203.0.113.10
  IdentityFile ~/.ssh/identities/staging
  IdentitiesOnly yes
  User deploy
  Port 2209

Host registry
  IdentityFile ~/.ssh/identities/registry
  IdentitiesOnly yes
  User admin

Host production
  HostName 198.51.100.42
  IdentityFile ~/.ssh/identities/production
  IdentitiesOnly yes
  User deploy
  Port 59865

Host *
  ServerAliveInterval 60
"#;
        let hosts = parse_content(content);

        let filtered: Vec<SshConfigHost> = hosts
            .into_iter()
            .filter(|h| {
                if h.hostname.is_none() {
                    return false;
                }
                let has_wildcard =
                    h.name.contains('*') || h.name.contains('?') || h.name.contains('!');
                !has_wildcard
            })
            .collect();

        assert_eq!(filtered.len(), 3);

        let github = filtered.iter().find(|h| h.name == "github").unwrap();
        assert_eq!(github.hostname, Some("github.com".to_string()));
        assert!(github.identity_file.is_some());

        let staging = filtered.iter().find(|h| h.name == "staging").unwrap();
        assert_eq!(staging.hostname, Some("203.0.113.10".to_string()));
        assert_eq!(staging.user, Some("deploy".to_string()));
        assert_eq!(staging.port, Some(2209));

        let production = filtered.iter().find(|h| h.name == "production").unwrap();
        assert_eq!(production.port, Some(59865));

        assert!(filtered.iter().all(|h| !h.name.contains('*')));
    }
}
