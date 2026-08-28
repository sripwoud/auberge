//! Fence on the shared unit scan the five systemd fences read the tree
//! through.
//!
//! `start_limit`, `service_directories`, `install_notifies_restart`,
//! `shutdown_exit_status` and `unit_ownership` all answer their question by
//! asking which unit a role installs and what its file says. That scan is the
//! shared premise underneath all five, so a scan that quietly stops reaching
//! somewhere does not fail: it shrinks the domain, and every fence over it
//! passes vacuously (#668). These are the scan's own assertions —
//! `tests/task_walker.rs` for the ansible task walk one layer down,
//! `tests/crate_source_walk.rs` for the crate's source.
//!
//! Anchored on the tree itself where the claim is about the fleet, and on a
//! written-out body where it is a postcondition of the parser. The section
//! rule is the second kind and matters most: every fence but `start_limit`
//! used to read a directive with `line.strip_prefix("Restart=")` over the whole
//! file, and the tree happens not to hold a `Restart=` in a section systemd
//! ignores — so the tree cannot falsify the claim that sections are tracked,
//! and a body written here can.

mod common;

use common::units::{FLEET_UNIT_FILES, Scope, directives, fleet_units, unit_configured_at};

/// One unit file out of the scan, by [`common::units::InstalledUnit::file_id`].
/// Absent is a hard stop: every assertion below is about a specific file, and
/// one that reads nothing would pass having looked at nothing.
fn file(id: &str) -> common::units::InstalledUnit {
    fleet_units()
        .into_iter()
        .find(|unit| unit.file_id() == id)
        .unwrap_or_else(|| panic!("the scan no longer finds `{id}`"))
}

/// The reach pin, under a name. It fires inside [`fleet_units`] so that every
/// fence inherits it rather than trusting the scan, which means its failure
/// surfaces in whichever fence happened to call first; this is the assertion
/// that says so out loud.
///
/// The one shape the pin cannot catch by itself is both halves going empty at
/// once. Emptying `FLEET_UNIT_FILES` alone fails inside it — every scanned file
/// lands in `unlisted` — and so does a scan that stops finding anything while
/// the list still holds 39 names. It is the pair that passes: nothing scanned
/// and nothing listed makes the difference empty in both directions, and all
/// five fences then run over an empty domain. That is what the emptiness check
/// below is for, and it is the only thing it is for.
#[test]
fn test_the_scan_is_pinned_to_the_files_it_finds() {
    assert!(
        !FLEET_UNIT_FILES.is_empty(),
        "FLEET_UNIT_FILES is empty, so a scan that found nothing compares two \
         empty sets and all five fences over this domain pass having seen nothing"
    );
    assert_eq!(
        fleet_units().len(),
        FLEET_UNIT_FILES.len(),
        "the scan and its declared reach disagree on how many unit files the \
         fleet installs"
    );
}

/// The rule the four copies did not have. A key is readable from the section it
/// is written under and from no other, so a fence asking `[Service]` for a
/// setting systemd reads from `[Install]` gets nothing rather than a value it
/// would then vouch for.
///
/// `caddy.service` is the anchor because it is one of the units the repo ships
/// from `files/` rather than `templates/`, so the same case proves the scan
/// reaches both directories.
#[test]
fn test_a_directive_is_read_only_from_the_section_it_is_written_under() {
    let caddy = file("caddy/caddy.service");
    assert_eq!(
        caddy.last_in("Install", "WantedBy"),
        Some("multi-user.target"),
        "caddy.service declares `WantedBy=multi-user.target` under `[Install]`"
    );
    assert_eq!(
        caddy.last_in("Service", "WantedBy"),
        None,
        "`WantedBy` is written under `[Install]`; reading it from `[Service]` is \
         how a section-blind scan comes to vouch for a directive systemd applies \
         nowhere"
    );
    assert_eq!(
        caddy.last_in("Service", "Restart"),
        Some("on-failure"),
        "the section rule must still find a directive that is where it belongs"
    );
}

/// The parser's own postconditions, on a body written here because the tree
/// holds no file that would falsify them: section headers, comments and blank
/// lines are not assignments, a key's section travels with it, and a value is
/// the file's own text with the whitespace around the `=` taken off and nothing
/// else.
#[test]
fn test_the_parser_reads_assignments_and_nothing_else() {
    let parsed = directives(
        "# a comment\n\
         [Unit]\n\
         Description = spaced out\n\
         \n\
         ; the other comment marker\n\
         [Service]\n\
         Restart=always\n\
         Environment=A=1\n\
         [Install]\n\
         Restart=always\n",
    );
    let seen: Vec<(&str, &str, &str)> = parsed
        .iter()
        .map(|d| (d.section.as_str(), d.key.as_str(), d.value.as_str()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("Unit", "Description", "spaced out"),
            ("Service", "Restart", "always"),
            ("Service", "Environment", "A=1"),
            ("Install", "Restart", "always"),
        ],
        "a comment, a blank line and a section header are not assignments, and \
         only the first `=` splits a line — `Environment=A=1` is one directive, \
         not a key of `Environment=A`"
    );
}

/// `last_in` is systemd's rule for a single-value setting and `all_in` is its
/// rule for a merged one, so the two cannot be swapped: reading
/// `ReadWritePaths` with `last_in` would drop every grant but the final line,
/// and reading `Restart` with `all_in` would hand a fence a value that was
/// overridden.
#[test]
fn test_repeated_assignments_are_merged_or_overridden_as_systemd_does_it() {
    let unit = common::units::InstalledUnit {
        role: "written-here".to_string(),
        name: "written-here.service".to_string(),
        scope: Scope::System,
        dropin: None,
        directives: directives(
            "[Service]\n\
             ReadWritePaths=/one\n\
             Restart=no\n\
             ReadWritePaths=/two\n\
             Restart=always\n",
        ),
    };
    assert_eq!(
        unit.all_in("Service", "ReadWritePaths"),
        vec!["/one", "/two"],
        "systemd merges repeated list-valued lines, and so must the reader"
    );
    assert_eq!(
        unit.last_in("Service", "Restart"),
        Some("always"),
        "the last assignment of a single-value setting is the live one"
    );
    assert_eq!(
        unit.sections_assigning("Restart"),
        vec!["Service", "Service"],
        "every assignment's section, so a setting written where systemd does not \
         read it can be named rather than silently believed"
    );
}

/// One [`common::units::InstalledUnit`] is one file. A drop-in's assignments
/// stay out of the unit file's list, because which file an assignment is in is
/// a claim `start_limit` makes: an adopted unit's drop-in has to pin the
/// `RestartSec` the limiter's reach is computed against, and a merged view
/// cannot tell that from the packaged unit having set it.
#[test]
fn test_a_drop_in_is_its_own_file_and_names_the_unit_it_refines() {
    let unit = file("caddy/caddy.service");
    let dropin = file("caddy/caddy.service.d/caddy-env.conf");

    assert_eq!(
        dropin.name, unit.name,
        "a drop-in's `<unit>.d/` directory names the unit it refines"
    );
    assert_eq!(dropin.dropin.as_deref(), Some("caddy-env.conf"));
    assert_eq!(dropin.file(), "caddy-env.conf");
    assert_eq!(unit.dropin, None);
    assert_eq!(unit.file(), "caddy.service");

    assert_eq!(
        dropin.all_in("Service", "Environment").len(),
        1,
        "caddy's drop-in exists to carry the DNS API token into the unit"
    );
    assert!(
        unit.all_in("Service", "Environment").is_empty(),
        "the drop-in's assignment must not appear in the unit file's own list; \
         it is the drop-in that carries the token, and a merged view would lose \
         which of the two files does"
    );
}

/// A unit the repo only drops in over yields drop-ins and no unit file, which
/// is how `start_limit` tells an adopted unit from a templated one. navidrome
/// is the case, and its `start-limit.conf` splits one decision across two
/// sections — the `[Unit]` window beside the `[Service]` delay the window's
/// reach is computed from.
#[test]
fn test_a_unit_the_repo_does_not_template_is_seen_through_its_drop_ins_alone() {
    let files: Vec<String> = fleet_units()
        .iter()
        .filter(|unit| unit.role == "navidrome")
        .map(|unit| unit.file_id())
        .collect();
    assert_eq!(
        files,
        vec![
            "navidrome/navidrome.service.d/memory.conf".to_string(),
            "navidrome/navidrome.service.d/start-limit.conf".to_string(),
        ],
        "the `.deb` ships navidrome's unit; the role writes only drop-ins over it"
    );

    let limit = file("navidrome/navidrome.service.d/start-limit.conf");
    assert_eq!(limit.last_in("Unit", "StartLimitIntervalSec"), Some("3600"));
    assert_eq!(limit.last_in("Service", "RestartSec"), Some("10"));
    assert_eq!(
        limit.last_in("Service", "StartLimitIntervalSec"),
        None,
        "systemd reads `StartLimitIntervalSec` from `[Unit]` and nowhere else, so \
         a scan that finds it under `[Service]` has stopped tracking sections"
    );
}

/// A unit written out inline is a unit. cockpit's drop-in has no `src:` at all
/// — the body is spelled in the task — and a scan reading `src` only would drop
/// `cockpit.socket` out of `unit_ownership`'s domain while its Meta went on
/// declaring it.
#[test]
fn test_a_body_written_inline_in_the_task_is_read_like_one_on_disk() {
    let dropin = file("cockpit/cockpit.socket.d/override.conf");
    assert_eq!(dropin.name, "cockpit.socket");
    assert_eq!(
        dropin.all_in("Socket", "ListenStream"),
        vec!["", "127.0.0.1:{{ cockpit_port }}"],
        "the empty first assignment is what clears systemd's packaged default \
         before the second narrows it to localhost; dropping either would read \
         as a socket bound somewhere it is not"
    );
}

/// The scan reaches both systemd managers. hermes is the fleet's only user
/// unit, and its dest resolves only as far as `{{ hermes_home }}` — an
/// unresolved *directory* is fine and a unit named through one is not, which is
/// the distinction that keeps a var-driven name from vanishing silently.
#[test]
fn test_the_scan_reaches_both_systemd_managers() {
    let user = file("hermes/hermes-gateway.service (user)");
    assert_eq!(user.scope, Scope::User);
    assert_eq!(user.last_in("Unit", "StartLimitBurst"), Some("30"));

    let system = file("caddy/caddy.service");
    assert_eq!(system.scope, Scope::System);
}

/// A `loop:` installs one unit per item, with the item substituted into both
/// the dest and the source — so the four paperless units are four units with
/// four bodies, not one body counted four times. Expanding the dest alone would
/// give four units all reading the first template.
#[test]
fn test_a_looped_task_installs_one_unit_per_item_with_its_own_body() {
    let execs: Vec<(String, String)> = fleet_units()
        .iter()
        .filter(|unit| unit.role == "paperless")
        .map(|unit| {
            (
                unit.name.clone(),
                unit.last_in("Service", "ExecStart")
                    .unwrap_or_else(|| panic!("{} must exec something", unit.name))
                    .to_string(),
            )
        })
        .collect();
    assert_eq!(execs.len(), 4, "paperless's one task installs four units");
    let distinct: std::collections::BTreeSet<&String> =
        execs.iter().map(|(_, exec)| exec).collect();
    assert_eq!(
        distinct.len(),
        4,
        "each looped item resolves its own `src`, so the four units exec four \
         different things: {execs:?}"
    );
}

/// A directive's value is the file's own text. Substituting through the role's
/// defaults is the reader's decision, made where it knows whether an unresolved
/// expression is a fact or a failure — `start_limit` reads numbers that never
/// hold one, while `service_directories` fails loud on a grant it cannot
/// resolve to a path.
#[test]
fn test_a_directive_value_is_the_files_own_text() {
    let unit = file("actual/actual.service");
    assert_eq!(
        unit.last_in("Service", "User"),
        Some("{{ actual_sys_user }}"),
        "an unresolved expression arrives verbatim; a scan that resolved it here \
         would decide for every reader at once"
    );
}

/// What counts as configuring a unit, and what does not. Written out rather
/// than read off the tree: the tree holds no dest that would falsify these, and
/// a resolver that admitted one of them would put a file that is not a unit
/// into all five fences.
#[test]
fn test_only_a_unit_file_or_a_drop_in_over_one_configures_a_unit() {
    assert_eq!(
        unit_configured_at("/etc/systemd/system/foo.service"),
        Some(("foo.service".to_string(), Scope::System, None))
    );
    assert_eq!(
        unit_configured_at("/home/someone/.config/systemd/user/foo.timer"),
        Some(("foo.timer".to_string(), Scope::User, None))
    );
    assert_eq!(
        unit_configured_at("/etc/systemd/system/foo.socket.d/bar.conf"),
        Some((
            "foo.socket".to_string(),
            Scope::System,
            Some("bar.conf".to_string())
        ))
    );

    for dest in [
        // Not a systemd unit directory.
        "/etc/caddy/foo.service",
        // A unit directory nests exactly one level, for drop-ins.
        "/etc/systemd/system/foo.service.d/deeper/bar.conf",
        // A drop-in directory holds `.conf` files; anything else there is not
        // read by systemd.
        "/etc/systemd/system/foo.service.d/README",
        // No unit type, so nothing addresses it as a unit.
        "/etc/systemd/system/foo.conf",
    ] {
        assert_eq!(
            unit_configured_at(dest),
            None,
            "`{dest}` configures no systemd unit"
        );
    }
}

/// The one shape the resolver refuses to guess at. A `loop:` driven by a
/// variable this scan cannot expand would leave a unit named
/// `{{ something }}.service`, which fails the unit-type test and drops out of
/// every fence without a word — the one way a new unit could enter the fleet
/// without entering any of them.
#[test]
#[should_panic(expected = "does not resolve")]
fn test_a_unit_named_by_an_unresolved_expression_is_a_hard_stop() {
    unit_configured_at("/etc/systemd/system/{{ app }}.service");
}
