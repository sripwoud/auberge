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
use clap::ValueEnum;
use eyre::Result;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Which of a Host's two addresses a connection takes.
///
/// Both the declared policy's answer and the value of the global `--via`
/// flag, because they are the same choice made at two different moments —
/// spelling them as two types would let one grow a third case the other
/// could not express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Via {
    /// The Host's declared `address`.
    Public,
    /// The Host's cached `tailscale_ip`.
    Tailnet,
}

impl Via {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Tailnet => "tailnet",
        }
    }
}

impl std::fmt::Display for Via {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const NO_OVERRIDE: u8 = 0;
const OVERRIDE_PUBLIC: u8 = 1;
const OVERRIDE_TAILNET: u8 = 2;

/// The global `--via` flag, set once from `main` before any command runs.
///
/// A process-wide static rather than a threaded parameter, the way
/// `output::set_verbose` already is: the flag is `global = true` on the clap
/// tree and reaches roughly twenty construction sites, and threading it would
/// undo exactly the collapse ADR-0067 performed.
static OVERRIDE: AtomicU8 = AtomicU8::new(NO_OVERRIDE);

/// Whether the override has actually decided a route this run. `--via` on a
/// command that connects to nothing is an operator error worth reporting, and
/// a flag with no observable effect is indistinguishable from one that
/// silently failed to apply — see [`ensure_override_reached_a_host`].
static OVERRIDE_REACHED_A_HOST: AtomicBool = AtomicBool::new(false);

/// Install the run's `--via` override. Called once, from `main`.
pub fn set_override(via: Option<Via>) {
    OVERRIDE.store(
        match via {
            None => NO_OVERRIDE,
            Some(Via::Public) => OVERRIDE_PUBLIC,
            Some(Via::Tailnet) => OVERRIDE_TAILNET,
        },
        Ordering::Relaxed,
    );
    OVERRIDE_REACHED_A_HOST.store(false, Ordering::Relaxed);
}

fn route_override() -> Option<Via> {
    match OVERRIDE.load(Ordering::Relaxed) {
        OVERRIDE_PUBLIC => Some(Via::Public),
        OVERRIDE_TAILNET => Some(Via::Tailnet),
        _ => None,
    }
}

/// Fails when `--via` was given to a command that resolved no Host route.
///
/// Called by `main` *after* the command, because whether a command routes is
/// not knowable before it runs — a static list of routing subcommands is the
/// kind of thing a new command forgets to join. The cost is a non-zero exit
/// after work that already succeeded; the alternative is a flag that reads as
/// applied and was not, which on `--via public` means believing you moved off
/// a route you are still on.
pub fn ensure_override_reached_a_host() -> Result<()> {
    let Some(via) = route_override() else {
        return Ok(());
    };
    if OVERRIDE_REACHED_A_HOST.load(Ordering::Relaxed) {
        return Ok(());
    }
    eyre::bail!(
        "--via {via} changed nothing: this command resolved no route to a Host. It applies \
         only to commands that connect to a host in hosts.toml — the command itself ran."
    )
}

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
    match route_override() {
        Some(via) => {
            OVERRIDE_REACHED_A_HOST.store(true, Ordering::Relaxed);
            route_over(host, key_path, via, &format!("--via {via}"))
        }
        None => declared(host, key_path),
    }
}

/// The route the roster declares, with no `--via` applied.
///
/// The generated ssh include is written from this and never from [`resolve`]:
/// `~/.ssh/config.d/auberge.conf` outlives the command that regenerated it
/// (ADR-0070 binds regeneration to every roster write, `--via` included), so
/// baking a per-invocation override into it would leave interactive
/// `ssh <name>` on a route nobody declared — the divergence #780 exists to
/// close, reintroduced by the flag meant to work around it.
pub fn declared(host: &Host, key_path: Option<PathBuf>) -> Result<Route> {
    let via = if host.prefer_tailnet {
        Via::Tailnet
    } else {
        Via::Public
    };
    route_over(host, key_path, via, "prefer_tailnet")
}

/// `chose` names what asked for this route, so the failure says whether to
/// fix the roster or drop the flag.
fn route_over(host: &Host, key_path: Option<PathBuf>, via: Via, chose: &str) -> Result<Route> {
    let address = match via {
        Via::Public => host.address.clone(),
        Via::Tailnet => host.tailscale_ip.clone().ok_or_else(|| {
            eyre::eyre!(
                "{chose} routes host '{}' over the tailnet, but it has no cached \
                 tailscale_ip; run `auberge --via public host detect-tailscale-ip {}` first",
                host.name,
                host.name
            )
        })?,
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

    /// Restores the process-wide override on drop, so a failing assertion
    /// cannot leak `--via` into whatever test runs next in this binary.
    /// Callers hold `output::TEST_LOCK`, the same lock the env-var guards
    /// there use, because this static is global for the same reason they are.
    struct OverrideGuard;

    impl OverrideGuard {
        fn set(via: Option<Via>) -> Self {
            set_override(via);
            Self
        }
    }

    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            set_override(None);
        }
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

    #[test]
    fn via_public_overrides_a_hosts_tailnet_policy() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(Some(Via::Public));
        let host = Host::fixture("auberge", Some("100.64.0.1")).preferring_tailnet();

        assert_eq!(resolve(&host, None).unwrap().address, "203.0.113.10");
    }

    #[test]
    fn via_tailnet_overrides_a_host_that_declared_nothing() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(Some(Via::Tailnet));
        let host = Host::fixture("vieille-auberge", Some("100.64.0.4"));

        assert_eq!(resolve(&host, None).unwrap().address, "100.64.0.4");
    }

    /// The override is per-invocation. `declared` is what the generated ssh
    /// include is written from, and it must answer the roster's decision even
    /// while a `--via` is in force — otherwise `auberge --via public host
    /// edit x` republishes every alias on the public address for good.
    #[test]
    fn declared_ignores_the_override() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(Some(Via::Public));
        let host = Host::fixture("auberge", Some("100.64.0.1")).preferring_tailnet();

        assert_eq!(declared(&host, None).unwrap().address, "100.64.0.1");
    }

    /// Strict in the override direction too, and the message says the flag
    /// asked rather than the roster, because the fix differs.
    #[test]
    fn via_tailnet_refuses_a_host_with_no_cached_address() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(Some(Via::Tailnet));

        let err = resolve(&Host::fixture("ruche", None), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--via tailnet"), "{err}");
        assert!(err.contains("host detect-tailscale-ip ruche"), "{err}");
    }

    #[test]
    fn an_override_that_decided_a_route_is_not_reported() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(Some(Via::Public));

        resolve(&host(), None).unwrap();
        assert!(ensure_override_reached_a_host().is_ok());
    }

    /// `--via` on a command that connects to nothing must not pass silently:
    /// a flag that reads as applied and was not is the failure mode #780 is
    /// made of.
    #[test]
    fn an_override_that_reached_no_host_is_reported() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(Some(Via::Tailnet));

        let err = ensure_override_reached_a_host().unwrap_err().to_string();
        assert!(err.contains("--via tailnet"), "{err}");
        assert!(err.contains("resolved no route"), "{err}");
    }

    /// And the include's own resolution must not count as reaching one:
    /// every roster write regenerates it, so a `host add` under `--via` would
    /// otherwise report the flag as applied when it decided nothing.
    #[test]
    fn declared_does_not_count_as_the_override_reaching_a_host() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(Some(Via::Public));

        declared(&host(), None).unwrap();
        assert!(ensure_override_reached_a_host().is_err());
    }

    #[test]
    fn no_override_is_never_reported() {
        let _lock = crate::output::TEST_LOCK.lock().unwrap();
        let _guard = OverrideGuard::set(None);

        assert!(ensure_override_reached_a_host().is_ok());
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
