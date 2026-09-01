use clap::ValueEnum;
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;

/// A Host's tailnet trust tier: ADR-0055's four, spelled bare (`agent`), which
/// is the vocabulary `ansible/roles/headscale/files/policy.hujson` declares
/// under `tagOwners`.
///
/// Closed on purpose. The policy is a `files/` asset — no Jinja, identical on
/// every tailnet — so which tiers exist is a static property of the repo, and
/// `tests/headscale_acl_policy.rs` holds this type and that file to each other.
/// Widening the tailnet's trust vocabulary is a decision made twice (ADR-0046),
/// never a typo.
///
/// **Why this check is static.** #767 asked to reject a value "the deployed
/// policy's `tagOwners` does not define". That is a different question, and not
/// one a roster edit can ask. Three checks exist, at three moments:
///
/// | check | question | when |
/// | --- | --- | --- |
/// | `validate_tag` (`commands::headscale`) | well-formed tag? | any `--tags` |
/// | this type | one of the fleet's tiers? | `hosts.toml` parse |
/// | headscale's `TagExists`, translated by `tag_node` | does the *loaded* policy name it? | `nodes tag` |
///
/// Only the middle one is answerable while writing a roster entry. `TagExists`
/// returns false while `pm.pol == nil` (ADR-0061), so it says "no" to a legal
/// tier whenever no policy is loaded — true of this tailnet for a day, and true
/// of any Host enrolled before its control plane exists. It also needs SSH to
/// the control-plane Host, which editing a local file must not. So the runtime
/// gate keeps its own owner where it fires, and the roster gets the static one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum TailnetTag {
    Trusted,
    Data,
    Agent,
    Standby,
}

impl TailnetTag {
    /// Every tier, in ADR-0055's order — the set the policy fence expects
    /// `tagOwners` to name, and the picker's item list.
    ///
    /// Hand-written, so it can drift from the variants it claims to enumerate:
    /// a fifth variant left out of here would leave the policy fence comparing
    /// four tiers to four and passing, while `hosts.toml` quietly accepted a
    /// fifth. `all_is_every_variant` pins it to what the `ValueEnum` derive
    /// generates from the real variant list.
    pub const ALL: [Self; 4] = [Self::Trusted, Self::Data, Self::Agent, Self::Standby];

    /// The bare tier name, which is also how it deserializes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Data => "data",
            Self::Agent => "agent",
            Self::Standby => "standby",
        }
    }

    /// The tag as headscale spells it: what rides `--tags` on a pre-auth key
    /// and what appears under `tagOwners`. The roster stores the bare tier
    /// because the `tag:` prefix is a constant the type already carries.
    ///
    /// Its caller today is `tests/headscale_acl_policy.rs`, which needs the
    /// prefixed form to compare this type against the policy's `tagOwners`.
    /// The production caller is #768's auto-mint. Named because a `pub` item
    /// with no production call site no longer trips `dead_code` (ADR-0046), so
    /// nothing else would say so.
    pub fn acl_tag(self) -> String {
        format!("tag:{}", self.as_str())
    }
}

impl std::fmt::Display for TailnetTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    pub address: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python_interpreter: Option<String>,
    #[serde(default = "default_become_method")]
    pub become_method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_ip: Option<String>,
    /// The Host's ADR-0055 trust tier. Deliberately not derived from `tags`:
    /// those are Ansible inventory groups (`all.children.<tag>`), read by
    /// `when: "'<x>' in group_names"` guards, so deriving one from the other
    /// would make "which roles run here" and "what this Host may reach on the
    /// network" the same declaration — adding a group would silently move a
    /// Host's trust.
    ///
    /// Optional because the roster predates the field and never covers every
    /// node: `lechuck` and `pixel-9a` have no `hosts.toml` entry and never
    /// will. `auberge host list` shows the tier so an unset one is visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailnet_tag: Option<TailnetTag>,
    /// Every key this binary's `Host` does not declare, captured verbatim and
    /// written back unchanged (#788, ADR-0068).
    ///
    /// A mutating subcommand (`add`/`edit`/`rename`/`detect-tailscale-ip`)
    /// reads the whole roster, rebuilds each `Host` it does not touch as a
    /// typed struct, and writes the whole file back. Before this field
    /// existed, a binary that predated a field silently dropped it on that
    /// round trip — reading succeeded, so nothing warned. `tailnet_tag`
    /// (#767) has already been lost this way once, and `prefer_tailnet`
    /// (#787) would make the next loss a silent route change rather than a
    /// missing label. `#[serde(flatten)]` collects whatever a future field
    /// looks like — before this binary has a name for it — into this map, so
    /// mutating a Host at that field's neighbours no longer requires the
    /// binary to already know the field exists.
    ///
    /// A call site that reconstructs a `Host` field-by-field (`commands::host
    /// ::run_host_edit`) must still carry this forward explicitly, the same
    /// way it already threads `python_interpreter`/`become_method` through
    /// prompts that never ask about them — flatten protects the (de)serialize
    /// boundary, not a struct literal that skips a field on purpose.
    #[serde(flatten)]
    pub unknown: toml::Table,
}

#[derive(Debug, Serialize, Deserialize)]
struct HostsConfig {
    hosts: Vec<Host>,
}

fn default_port() -> u16 {
    22
}

fn default_become_method() -> String {
    "sudo".to_string()
}

/// The Hosts whose config answers a serving gate: the ADR-0051 shape — config
/// alone answers it, and a blank value is no answer — read through ADR-0058's
/// host-scoped view, so a `[hosts.<name>]` override decides for that Host.
///
/// Zero and several answers mean different things per gate, so both come back
/// as they are and the caller says which is a problem.
pub fn serving_hosts<'a>(
    hosts: &'a [Host],
    config: &crate::config::Config,
    gate_key: &str,
) -> Vec<&'a Host> {
    hosts
        .iter()
        .filter(|h| {
            config
                .get_for_host(gate_key, Some(&h.name))
                .is_some_and(|v| !v.trim().is_empty())
        })
        .collect()
}

impl Host {
    /// A roster entry with only the two fields the gate lookups read, so a
    /// unit test elsewhere in the crate does not restate the other eight.
    /// Test-only, like `Config::from_toml_str`.
    #[cfg(test)]
    pub fn fixture(name: &str, tailscale_ip: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            address: "203.0.113.10".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: tailscale_ip.map(str::to_string),
            tailnet_tag: None,
            unknown: toml::Table::new(),
        }
    }
}

pub struct HostManager;

impl HostManager {
    pub fn config_path() -> Result<PathBuf> {
        let config_dir = crate::config::Config::config_dir()?;
        Ok(config_dir.join("hosts.toml"))
    }

    pub fn load_hosts() -> Result<Vec<Host>> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(&config_path)
            .wrap_err_with(|| format!("Failed to read hosts config: {}", config_path.display()))?;

        let config: HostsConfig =
            toml::from_str(&contents).wrap_err("Failed to parse hosts.toml")?;

        Ok(config.hosts)
    }

    pub fn save_hosts(hosts: &[Host]) -> Result<()> {
        let config_path = Self::config_path()?;

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).wrap_err_with(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let config = HostsConfig {
            hosts: hosts.to_vec(),
        };

        let contents =
            toml::to_string_pretty(&config).wrap_err("Failed to serialize hosts config")?;

        fs::write(&config_path, contents)
            .wrap_err_with(|| format!("Failed to write hosts config: {}", config_path.display()))?;

        Ok(())
    }

    pub fn add_host(host: Host) -> Result<()> {
        let mut hosts = Self::load_hosts()?;

        if hosts.iter().any(|h| h.name == host.name) {
            eyre::bail!("Host '{}' already exists", host.name);
        }

        hosts.push(host);
        Self::save_hosts(&hosts)?;

        Ok(())
    }

    pub fn remove_host(name: &str) -> Result<()> {
        let mut hosts = Self::load_hosts()?;

        let original_len = hosts.len();
        hosts.retain(|h| h.name != name);

        if hosts.len() == original_len {
            eyre::bail!("Host '{}' not found", name);
        }

        Self::save_hosts(&hosts)?;

        Ok(())
    }

    pub fn get_host(name: &str) -> Result<Host> {
        let hosts = Self::load_hosts()?;

        hosts
            .into_iter()
            .find(|h| h.name == name)
            .ok_or_else(|| eyre::eyre!("Host '{}' not found", name))
    }

    pub fn update_host(name: &str, updated_host: Host) -> Result<()> {
        let mut hosts = Self::load_hosts()?;

        let host = hosts
            .iter_mut()
            .find(|h| h.name == name)
            .ok_or_else(|| eyre::eyre!("Host '{}' not found", name))?;

        *host = updated_host;

        Self::save_hosts(&hosts)?;

        Ok(())
    }

    pub fn list_hosts_filtered(tags: Option<Vec<String>>) -> Result<Vec<Host>> {
        let hosts = Self::load_hosts()?;

        if let Some(filter_tags) = tags {
            Ok(hosts
                .into_iter()
                .filter(|h| filter_tags.iter().any(|tag| h.tags.contains(tag)))
                .collect())
        } else {
            Ok(hosts)
        }
    }

    pub fn is_tty() -> bool {
        std::io::stdin().is_terminal()
    }
}

/// How a command names its host, for the error raised when no picker can be
/// drawn. Commands take the host either as a flag or as a positional argument;
/// naming the wrong one is worse than naming none.
pub const HOST_FLAG: &str = "-H <host>";
pub const HOST_POSITIONAL: &str = "the host name as an argument";

pub fn host_choice(argument: &str) -> crate::prompt::Choice {
    crate::prompt::Choice::new("host")
        .resolved_by(argument)
        .populated_by("auberge host add")
}

pub fn select_or_arg(arg: Option<String>, argument: &str) -> eyre::Result<Host> {
    match arg {
        Some(name) => HostManager::get_host(&name),
        None => crate::prompt::select_item(
            &HostManager::load_hosts()?,
            |h: &Host| format!("{} ({}:{})", h.name, h.address, h.port),
            host_choice(argument),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn names<'a>(hosts: &[&'a Host]) -> Vec<&'a str> {
        hosts.iter().map(|h| h.name.as_str()).collect()
    }

    #[test]
    fn serving_hosts_counts_every_host_a_fleet_wide_answer_reaches() {
        let hosts = [Host::fixture("auberge", None), Host::fixture("ruche", None)];
        let config = Config::from_toml_str(r#"blocky_subdomain = "dns""#).unwrap();
        assert_eq!(
            names(&serving_hosts(&hosts, &config, "blocky_subdomain")),
            vec!["auberge", "ruche"]
        );
    }

    #[test]
    fn serving_hosts_drops_a_host_that_blanked_the_gate() {
        let hosts = [Host::fixture("auberge", None), Host::fixture("ruche", None)];
        let config = Config::from_toml_str(
            r#"
blocky_subdomain = "dns"

[hosts.ruche]
blocky_subdomain = ""
"#,
        )
        .unwrap();
        assert_eq!(
            names(&serving_hosts(&hosts, &config, "blocky_subdomain")),
            vec!["auberge"]
        );
    }

    #[test]
    fn serving_hosts_counts_a_host_that_answers_only_in_its_own_table() {
        let hosts = [Host::fixture("auberge", None), Host::fixture("ruche", None)];
        let config = Config::from_toml_str(
            r#"
[hosts.ruche]
blocky_subdomain = "dns"
"#,
        )
        .unwrap();
        assert_eq!(
            names(&serving_hosts(&hosts, &config, "blocky_subdomain")),
            vec!["ruche"]
        );
    }

    #[test]
    fn serving_hosts_is_empty_when_nothing_answers_the_gate() {
        let hosts = [Host::fixture("auberge", None)];
        let config = Config::from_toml_str(r#"domain = "example.com""#).unwrap();
        assert!(serving_hosts(&hosts, &config, "blocky_subdomain").is_empty());
    }

    #[test]
    fn test_host_serialization() {
        let host = Host {
            name: "test".to_string(),
            address: "192.168.1.1".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec!["production".to_string()],
            description: Some("Test host".to_string()),
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
            tailnet_tag: None,
            unknown: toml::Table::new(),
        };

        let config = HostsConfig { hosts: vec![host] };

        let toml_str = toml::to_string(&config).unwrap();
        let parsed: HostsConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.hosts.len(), 1);
        assert_eq!(parsed.hosts[0].name, "test");
        assert_eq!(parsed.hosts[0].port, 22);
    }

    #[test]
    fn test_default_values() {
        let toml_str = r#"
            [[hosts]]
            name = "minimal"
            address = "1.2.3.4"
            user = "root"
        "#;

        let config: HostsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hosts[0].port, 22);
        assert_eq!(config.hosts[0].become_method, "sudo");
        assert!(config.hosts[0].tags.is_empty());
        assert!(config.hosts[0].tailscale_ip.is_none());
        assert!(config.hosts[0].tailnet_tag.is_none());
        assert!(config.hosts[0].unknown.is_empty());
    }

    #[test]
    fn test_tailscale_ip_round_trip() {
        let host = Host {
            name: "vps".to_string(),
            address: "203.0.113.10".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: Some("100.64.0.5".to_string()),
            tailnet_tag: None,
            unknown: toml::Table::new(),
        };

        let serialized = toml::to_string(&HostsConfig { hosts: vec![host] }).unwrap();
        assert!(serialized.contains("tailscale_ip = \"100.64.0.5\""));

        let parsed: HostsConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.hosts[0].tailscale_ip.as_deref(), Some("100.64.0.5"));
    }

    fn host_toml(body: &str) -> Result<HostsConfig> {
        Ok(toml::from_str(&format!(
            "[[hosts]]\nname = \"vps\"\naddress = \"203.0.113.10\"\nuser = \"admin\"\n{body}"
        ))?)
    }

    /// #788: a binary whose `Host` predates a field must not delete it on a
    /// load/save round trip — `tailnet_tag` (#767) was lost this way once,
    /// and the next loss (`prefer_tailnet`, #787) would be a silent route
    /// change rather than a missing label.
    ///
    /// The sentinel key is checked against every field [`Host`] currently
    /// serializes — derived from a fully-populated instance rather than a
    /// hand-typed list — so a future field that happens to share the
    /// sentinel's name fails this assertion instead of letting the test pass
    /// without exercising the guarantee.
    #[test]
    fn an_unrecognized_key_survives_a_load_and_save_round_trip() {
        let full = Host {
            name: "vps".to_string(),
            address: "203.0.113.10".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: Some("~/.ssh/id_ed25519".to_string()),
            tags: vec!["production".to_string()],
            description: Some("full fixture".to_string()),
            python_interpreter: Some("/usr/bin/python3".to_string()),
            become_method: "sudo".to_string(),
            tailscale_ip: Some("100.64.0.5".to_string()),
            tailnet_tag: Some(TailnetTag::Trusted),
            unknown: toml::Table::new(),
        };
        let known_fields: Vec<String> = toml::Value::try_from(full)
            .unwrap()
            .as_table()
            .unwrap()
            .keys()
            .cloned()
            .collect();

        let sentinel = "a_field_this_binary_has_never_heard_of";
        assert!(
            !known_fields.contains(&sentinel.to_string()),
            "sentinel collides with a real Host field {known_fields:?}; rename the sentinel"
        );

        let config = host_toml(&format!("{sentinel} = \"kept\"")).unwrap();
        let saved = toml::to_string(&config).unwrap();

        assert!(
            saved.contains(&format!("{sentinel} = \"kept\"")),
            "an unrecognized key must survive a load/save round trip: {saved}"
        );
    }

    /// [`TailnetTag::ALL`] is a literal; the `ValueEnum` derive generates its
    /// list from the variants themselves. Equality here is what lets every
    /// other assertion — the policy fence included — treat `ALL` as "every
    /// tier" rather than "four tiers somebody remembered".
    #[test]
    fn all_is_every_variant() {
        assert_eq!(TailnetTag::ALL.as_slice(), TailnetTag::value_variants());
    }

    /// Three spellings of one name — `as_str`/`Display`, serde's
    /// `rename_all = "lowercase"`, and clap's derived value name — and a tier
    /// the CLI accepts must be one the roster file parses. Pinned rather than
    /// assumed: they agree today only because every variant is a single word.
    #[test]
    fn the_cli_the_roster_and_display_spell_a_tier_alike() {
        for tier in TailnetTag::ALL {
            let cli = tier
                .to_possible_value()
                .expect("every tier must be selectable on the command line");
            assert_eq!(cli.get_name(), tier.as_str(), "{tier}");
        }
    }

    #[test]
    fn a_tier_round_trips_through_the_roster_file() {
        for tier in TailnetTag::ALL {
            let parsed = host_toml(&format!("tailnet_tag = \"{tier}\""))
                .unwrap_or_else(|e| panic!("{tier} must parse: {e}"));
            assert_eq!(parsed.hosts[0].tailnet_tag, Some(tier));

            let written = toml::to_string(&parsed).unwrap();
            assert!(
                written.contains(&format!("tailnet_tag = \"{tier}\"")),
                "{tier} must be written back as it was read: {written}"
            );
        }
    }

    /// The check #767 asked for, at the only moment it can be made: a value the
    /// tailnet's tier vocabulary does not name is refused, and the refusal names
    /// every value that would have worked.
    ///
    /// Static and local by construction. The runtime alternative — headscale's
    /// `TagExists` against the deployed policy — cannot answer here: it returns
    /// false whenever no policy is loaded (ADR-0061), so it would reject a legal
    /// tier on any tailnet before its first policy deploy, and it would need SSH
    /// to the control-plane Host to answer at all.
    #[test]
    fn an_undeclared_tier_is_refused_and_the_refusal_names_the_legal_ones() {
        let err = host_toml(r#"tailnet_tag = "yolo""#)
            .expect_err("a tier the policy does not declare must not parse")
            .to_string();
        for tier in TailnetTag::ALL {
            assert!(
                err.contains(tier.as_str()),
                "the error must name {tier} as a legal value: {err}"
            );
        }
    }

    /// `tag:` is a constant of the wire format, not of the roster: the file
    /// spells `agent`, headscale is handed `tag:agent`.
    #[test]
    fn the_acl_tag_is_the_tier_prefixed() {
        assert_eq!(TailnetTag::Agent.acl_tag(), "tag:agent");
        assert_eq!(
            TailnetTag::ALL.map(TailnetTag::acl_tag).to_vec(),
            ["tag:trusted", "tag:data", "tag:agent", "tag:standby"]
        );
    }

    #[test]
    fn test_tailscale_ip_omitted_when_none() {
        let host = Host {
            name: "vps".to_string(),
            address: "203.0.113.10".to_string(),
            user: "admin".to_string(),
            port: 22,
            ssh_key: None,
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
            tailnet_tag: None,
            unknown: toml::Table::new(),
        };

        let serialized = toml::to_string(&HostsConfig { hosts: vec![host] }).unwrap();
        assert!(!serialized.contains("tailscale_ip"));
    }
}
