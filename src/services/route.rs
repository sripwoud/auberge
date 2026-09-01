//! #780's resolved Route to a Host — every ssh, scp, rsync and ansible
//! connection the CLI makes collapses through here, so a future policy
//! (#787's `prefer_tailnet`) has one seam to change rather than six.
//!
//! `Host` stays the declaration (`hosts.toml`); a `Route` is what a consumer
//! takes to actually reach it — nothing outside this module needs a `Host` in
//! order to connect.
//!
//! #787 gave the seam its policy: a Host declaring `prefer_tailnet` is
//! reached at its `tailscale_ip`. Two other questions look like the same one
//! and are not, so both are named here rather than left to a caller to
//! improvise from `Host::address`:
//!
//! | question | answer |
//! | --- | --- |
//! | where does this CLI connect? | [`resolve`] |
//! | what does the world reach this Host at? | [`public_address`] |
//! | what may a peer arrive *from*? | [`peer_addresses`] |
//!
//! Conflating the first two publishes a CGNAT address in public DNS;
//! conflating the first and third bans the recovery route from fail2ban's
//! allowlist exactly when it is needed.

use crate::hosts::Host;
use eyre::Result;
use std::path::PathBuf;

/// Everything needed to reach a Host.
///
/// `key_path` is `None` for a consumer that never opens an identity file of
/// its own: ansible resolves its own connection independently of this seam,
/// and the generated ssh config's `IdentityFile` line keeps its own
/// tilde-preserving logic (`services::ssh_include::identity_file`) rather
/// than a real, filesystem-checked path — the two answer different
/// questions ("what to write" vs. "what to open") even for the same Host.
///
/// `alias` is always the Host's name today; #785 wires it up as ssh's
/// `HostKeyAlias`, so identity stops being a function of the address below
/// it.
#[derive(Debug, Clone)]
pub struct Route {
    pub address: String,
    pub port: u16,
    pub user: String,
    pub key_path: Option<PathBuf>,
    pub alias: String,
}

/// The one place `Host::address` is read to decide how to reach a Host.
///
/// `prefer_tailnet` decides, and it decides **strictly**: a Host carrying the
/// policy is reached at its `tailscale_ip` or not at all. There is no fall
/// back to the public address on a tailnet that does not answer —
/// `auberge-backup.service` runs `backup sync … --quiet`, so a warning in the
/// nightly path is structurally invisible, and silently taking the public
/// route *is* the incident #780 comes from.
///
/// Fallible because the policy can outlive its fact: `Host::validate` refuses
/// the combination at every roster write, so reaching the `bail` here means
/// the file was edited past that gate, and failing loud with the remedy named
/// beats connecting somewhere nobody chose.
///
/// `key_path` is already resolved by the caller — this function does no key
/// derivation of its own, so a caller with no identity file of its own
/// (ansible's inventory conversion, the generated ssh include) passes `None`.
pub fn resolve(host: &Host, key_path: Option<PathBuf>) -> Result<Route> {
    let address = if host.prefer_tailnet {
        host.tailscale_ip.clone().ok_or_else(|| {
            eyre::eyre!(
                "host '{}' sets prefer_tailnet but has no tailscale_ip to route to; run \
                 `auberge --via public host detect-tailscale-ip {}` first",
                host.name,
                host.name
            )
        })?
    } else {
        host.address.clone()
    };

    Ok(Route {
        address,
        port: host.port,
        user: host.user.clone(),
        key_path,
        alias: host.name.clone(),
    })
}

/// The address the world reaches this Host at — what DNS publishes and what
/// `--via public` falls back to — regardless of where the CLI connects.
///
/// Not a [`Route`], and deliberately not spelled `host.address` at its call
/// sites: `ansible_host` in the Inventory carries both meanings, and a reader
/// who assumes the routing one publishes `100.64.0.1` as a public A record.
/// Naming the question is what keeps the two apart (#787).
pub fn public_address(host: &Host) -> String {
    host.address.clone()
}

/// Every address a fleet peer may arrive *from*, which is not the question
/// [`resolve`] answers.
///
/// fail2ban must never ban an Inventory peer (#582), and once a Host routes
/// over the tailnet its peers arrive at its sshd from their tailnet
/// addresses, while `--via public` recovery still arrives from their public
/// ones. Ignoring only the addresses currently in use would make the recovery
/// route bannable at exactly the moment it is needed, so both are listed.
pub fn peer_addresses(hosts: &[Host]) -> Vec<String> {
    hosts
        .iter()
        .flat_map(|host| std::iter::once(host.address.clone()).chain(host.tailscale_ip.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        Host::fixture("auberge", None)
    }

    #[test]
    fn resolve_copies_the_hosts_address_port_and_user() {
        let route = resolve(&host(), None).unwrap();
        assert_eq!(route.address, "203.0.113.10");
        assert_eq!(route.port, 22);
        assert_eq!(route.user, "admin");
    }

    #[test]
    fn resolve_names_the_alias_after_the_host() {
        assert_eq!(resolve(&host(), None).unwrap().alias, "auberge");
    }

    #[test]
    fn resolve_carries_the_key_path_it_is_given() {
        let key = PathBuf::from("/tmp/key");
        assert_eq!(
            resolve(&host(), Some(key.clone())).unwrap().key_path,
            Some(key)
        );
    }

    #[test]
    fn resolve_is_none_when_no_key_path_is_given() {
        assert_eq!(resolve(&host(), None).unwrap().key_path, None);
    }

    /// The fact is not the policy: `vieille-auberge` caches a tailnet address
    /// and must keep being reached over the public one. #784 asserted this
    /// while no policy existed; it has to keep holding now that one does.
    #[test]
    fn resolve_ignores_a_tailscale_ip_the_host_has_not_opted_into() {
        let host = Host::fixture("vieille-auberge", Some("100.64.0.4"));
        assert_eq!(resolve(&host, None).unwrap().address, host.address);
    }

    #[test]
    fn resolve_takes_the_tailnet_address_when_the_host_prefers_it() {
        let host = Host::fixture("auberge", Some("100.64.0.1")).preferring_tailnet();
        assert_eq!(resolve(&host, None).unwrap().address, "100.64.0.1");
    }

    /// Everything but the address stays the declaration's: a route change must
    /// not move the port, the user, or — since #785 — the host-key identity.
    #[test]
    fn a_tailnet_route_keeps_the_hosts_port_user_and_identity() {
        let host = Host::fixture("auberge", Some("100.64.0.1")).preferring_tailnet();
        let route = resolve(&host, None).unwrap();
        assert_eq!(route.port, 22);
        assert_eq!(route.user, "admin");
        assert_eq!(route.alias, "auberge");
    }

    /// No fallback. A policy whose fact has gone missing fails the command
    /// and names the fix, rather than quietly reverting to the public route.
    #[test]
    fn resolve_refuses_a_policy_with_no_address_to_route_to() {
        let mut host = Host::fixture("ruche", None).preferring_tailnet();
        host.tailscale_ip = None;

        let err = resolve(&host, None).unwrap_err().to_string();
        assert!(err.contains("prefer_tailnet"), "{err}");
        assert!(err.contains("host detect-tailscale-ip ruche"), "{err}");
    }

    #[test]
    fn public_address_stays_public_under_the_tailnet_policy() {
        let host = Host::fixture("auberge", Some("100.64.0.1")).preferring_tailnet();
        assert_eq!(public_address(&host), "203.0.113.10");
        assert_ne!(public_address(&host), resolve(&host, None).unwrap().address);
    }

    /// Both addresses, always: a Host that routes over the tailnet arrives
    /// from `100.64.0.1`, and the same Host under `--via public` arrives from
    /// `203.0.113.10`. fail2ban has to ignore whichever one shows up.
    #[test]
    fn peer_addresses_lists_a_hosts_public_and_tailnet_addresses() {
        let hosts = [
            Host::fixture("auberge", Some("100.64.0.1")).preferring_tailnet(),
            Host::fixture("lechuck", None),
        ];
        assert_eq!(
            peer_addresses(&hosts),
            vec!["203.0.113.10", "100.64.0.1", "203.0.113.10"]
        );
    }
}
