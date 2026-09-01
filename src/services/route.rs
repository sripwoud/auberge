//! #780's resolved Route to a Host — every ssh, scp, rsync and ansible
//! connection the CLI makes collapses through here, so a future policy
//! (#787's `prefer_tailnet`) has one seam to change rather than six.
//!
//! `Host` stays the declaration (`hosts.toml`); a `Route` is what a consumer
//! takes to actually reach it — nothing outside this module needs a `Host` in
//! order to connect. This slice resolves the public address unconditionally:
//! no policy, no `prefer_tailnet` yet. Only the seam exists now.

use crate::hosts::Host;
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
/// Unconditional: always the public address, until #787 adds a policy.
/// `key_path` is already resolved by the caller — this function does no key
/// derivation or validation of its own, so it stays infallible even for a
/// caller (ansible's inventory conversion, the generated ssh include) that
/// has no key to give and passes `None`.
pub fn resolve(host: &Host, key_path: Option<PathBuf>) -> Route {
    Route {
        address: host.address.clone(),
        port: host.port,
        user: host.user.clone(),
        key_path,
        alias: host.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        Host::fixture("auberge", None)
    }

    #[test]
    fn resolve_copies_the_hosts_address_port_and_user() {
        let route = resolve(&host(), None);
        assert_eq!(route.address, "203.0.113.10");
        assert_eq!(route.port, 22);
        assert_eq!(route.user, "admin");
    }

    #[test]
    fn resolve_names_the_alias_after_the_host() {
        assert_eq!(resolve(&host(), None).alias, "auberge");
    }

    #[test]
    fn resolve_carries_the_key_path_it_is_given() {
        let key = PathBuf::from("/tmp/key");
        assert_eq!(resolve(&host(), Some(key.clone())).key_path, Some(key));
    }

    #[test]
    fn resolve_is_none_when_no_key_path_is_given() {
        assert_eq!(resolve(&host(), None).key_path, None);
    }

    /// This slice's whole point: presence of `tailscale_ip` must not change
    /// what `resolve` answers — no policy exists yet (#787).
    #[test]
    fn resolve_ignores_tailscale_ip_until_787_adds_the_policy() {
        let host = Host::fixture("auberge", Some("100.64.0.5"));
        assert_eq!(resolve(&host, None).address, host.address);
    }
}
