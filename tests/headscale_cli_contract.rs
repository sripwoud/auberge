//! #707: headscale's command line is a contract, and a contract belongs to one
//! release.
//!
//! `src/commands/headscale.rs` spells remote command lines and deserialises the
//! JSON they print. Both are properties of the headscale binary on the Host,
//! which is the binary `ansible/playbooks/headscale.meta.yml` pins and Renovate
//! bumps. Nothing tied the two together, so Renovate walked 0.25 → 0.29
//! (#427/#436/#437/#445) across three breaking changes to that contract — a
//! `--user` that became a `uint` id, node JSON that was never protojson, and an
//! empty listing that prints `null` — and the suite stayed green throughout,
//! because a mocked ssh layer answers whatever the test staged.
//!
//! A test cannot reach the remote binary, so it cannot check the contract. What
//! it can do is refuse to let the pin move without a human reading the release
//! it moved to: the module names the version it was verified against, and this
//! fails the build when the App Version disagrees.
//!
//! The version is *imported*, not scraped out of the source: a fence wanting one
//! crate item reaches it with `use` (ADR-0046).

use auberge::commands::headscale::VERIFIED_CLI_VERSION;
use auberge::playbook_meta::declared_app_versions;

mod common;

use common::playbooks_dir;

/// The App whose CLI this crate drives.
const APP: &str = "headscale";

fn pinned_app_version() -> String {
    declared_app_versions(&playbooks_dir())
        .expect("playbook metas must parse")
        .into_iter()
        .find(|(app, _)| app == APP)
        .unwrap_or_else(|| panic!("{APP}.meta.yml must declare an App Version (ADR-0017)"))
        .1
        .value
}

#[test]
fn the_verified_cli_version_is_the_one_the_playbook_pins() {
    let pinned = pinned_app_version();
    assert_eq!(
        VERIFIED_CLI_VERSION, pinned,
        "\n\
         `auberge headscale` was verified against headscale {VERIFIED_CLI_VERSION}, but \
         ansible/playbooks/{APP}.meta.yml now deploys {pinned}.\n\
         Read the new release's CLI surface before moving the const — \
         `headscale users create --help`, `preauthkeys create --help`, \
         `users destroy --help`, `auth register --help` — plus the JSON its \
         `-o json` prints, and update \
         src/commands/headscale.rs to match. This is the check that was missing \
         when 0.25 → 0.29 broke add-user (#707)."
    );
}

/// Without this the assertion above passes by comparing two things it found,
/// whether or not either is the thing it meant — the failure mode a fence over
/// a lookup has, and the one #686 caught by asserting the domain was reached.
#[test]
fn the_pin_this_fence_reads_is_a_real_declared_app_version() {
    let versions = declared_app_versions(&playbooks_dir()).expect("playbook metas must parse");
    let (_, pin) = versions
        .iter()
        .find(|(app, _)| app == APP)
        .expect("headscale must be among the declared App Versions");

    assert_eq!(pin.dep_name, "juanfont/headscale");
    assert_eq!(pin.datasource, "github-releases");
    assert!(
        !pin.value.is_empty(),
        "an empty pin would match an empty const"
    );
    assert!(
        !VERIFIED_CLI_VERSION.is_empty(),
        "an empty const would match an empty pin"
    );
}
