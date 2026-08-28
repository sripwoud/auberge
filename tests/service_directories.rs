//! Fleet-wide guards on the directories roles create for their service users —
//! guard (i), that each unit's write access is exactly what a human declared it
//! to be, and guard (ii), that the Backup Recipe captures a directory precisely
//! when its declared kind says it is data (#621's numbering, kept below; #624).
//!
//! Both guards started life role-scoped in `tests/grimmory_role.rs` (#621), and
//! the scan that tried to generalize them found why they cannot generalize on
//! their own: 13 service-owned directories across the fleet are unwritable by
//! their unit, and every one is deliberate least privilege (ADR-0033). What the
//! repo cannot infer it must be told, so this is the declared regime of
//! ADR-0028: a computed set matched against a classification a human vouches
//! for, by equality in both directions, so a new directory fails the build
//! until it is classified and a stale declaration fails until it is removed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde_yaml::Value;

mod common;

use common::units::{InstalledUnit, Scope, fleet_units};
use common::{all_roles, defaults, field, playbooks_dir, resolve, role_tasks, strings};

/// A unit whose `ReadWritePaths` is the service's whole writable world --
/// which is only true under `ProtectSystem=strict`, so only those units carry
/// any writability fact this fence can assert.
struct StrictUnit {
    name: String,
    user: String,
    /// Every `ReadWritePaths=` entry, resolved; systemd merges repeated lines,
    /// so this does too. Empty is a real grant set, not an error: liquidsoap
    /// is strict with a `CacheDirectory=` and nothing else.
    grants: Vec<String>,
}

/// The role's strict units, out of the system manager's unit files the shared
/// scan found.
///
/// Drop-ins are filtered out rather than merged: what a drop-in refines was
/// installed by something else (apt's navidrome, icecast), and the sandbox it
/// runs under is not in the repo. Every directive is read from `[Service]`,
/// which is the only section systemd applies a sandbox from — a
/// `ProtectSystem=` under `[Unit]` sandboxes nothing, and reading one as strict
/// would have this fence assert grants against a unit that has none.
fn strict_units(
    installed: &[InstalledUnit],
    role: &str,
    vars: &BTreeMap<String, String>,
) -> Vec<StrictUnit> {
    let mut units = Vec::new();
    for unit in installed
        .iter()
        .filter(|unit| unit.role == role && unit.scope == Scope::System && unit.dropin.is_none())
    {
        let directive = |key: &str| {
            unit.last_in("Service", key)
                .map(|value| resolve(value, vars))
        };
        if directive("ProtectSystem").as_deref() != Some("strict") {
            continue;
        }
        let name = unit.name.clone();
        let grants: Vec<String> = unit
            .all_in("Service", "ReadWritePaths")
            .into_iter()
            .flat_map(|value| {
                resolve(value, vars)
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
        for grant in &grants {
            assert!(
                grant.starts_with('/'),
                "{role}: `{name}` grants `{grant}`, which the role's defaults \
                 cannot resolve to a path; a grant this fence cannot see would \
                 fail open, so it fails loud instead"
            );
        }
        units.push(StrictUnit {
            name,
            user: directive("User").unwrap_or_else(|| "root".to_string()),
            grants,
        });
    }
    units
}

/// The users a role hands directories to: whoever its strict units run as,
/// plus the `<role>_sys_user` convention for the Apps whose serving unit no
/// role templates (navidrome's apt unit, calibre's unconfined one, yourls's
/// php-fpm).
fn service_users(
    role: &str,
    vars: &BTreeMap<String, String>,
    units: &[StrictUnit],
) -> BTreeSet<String> {
    let mut users: BTreeSet<String> = units.iter().map(|unit| unit.user.clone()).collect();
    if let Some(user) = vars.get(&format!("{role}_sys_user")) {
        users.insert(user.clone());
    }
    users
}

/// Every directory the role creates and hands to a service user, `loop:` and
/// `with_items:` expanded, resolved through the role's defaults. An owner the
/// defaults cannot resolve (`{{ ansible_user }}`, a filter expression) is by
/// construction not a service user and drops out on the comparison.
fn created_directories(
    role: &str,
    vars: &BTreeMap<String, String>,
    users: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    for task in role_tasks(role) {
        let Some(args) = field(&task.body, "ansible.builtin.file").and_then(Value::as_mapping)
        else {
            continue;
        };
        if field(args, "state").and_then(Value::as_str) != Some("directory") {
            continue;
        }
        let Some(owner) = field(args, "owner").and_then(Value::as_str) else {
            continue;
        };
        let Some(path) = ["path", "dest", "name"]
            .iter()
            .find_map(|key| field(args, key).and_then(Value::as_str))
        else {
            continue;
        };
        let mut items = strings(field(&task.body, "loop"));
        items.extend(strings(field(&task.body, "with_items")));
        let expansions: Vec<String> = if items.is_empty() {
            vec![path.to_string()]
        } else {
            items
                .iter()
                .map(|item| path.replace("{{ item }}", item))
                .collect()
        };
        for path in expansions {
            if users.contains(&resolve(owner, vars)) {
                dirs.insert(resolve(&path, vars));
            }
        }
    }
    dirs
}

/// Every Backup Recipe's paths, keyed by App: the declared `paths` plus every
/// parameter's `adds_paths`, because what a parameter gates is still a path
/// the Recipe can push and a restore can put back (ADR-0026).
fn recipes() -> BTreeMap<String, Vec<String>> {
    let mut recipes = BTreeMap::new();
    for entry in fs::read_dir(playbooks_dir()).expect("ansible/playbooks must exist") {
        let path = entry.expect("a playbook entry must be readable").path();
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".meta.yml"))
        else {
            continue;
        };
        let raw = fs::read_to_string(&path).expect("a meta file must be readable");
        let meta: Value = serde_yaml::from_str(&raw)
            .unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
        let Some(backup) = meta.get("backup") else {
            continue;
        };
        let mut paths = strings(backup.get("paths"));
        if let Some(parameters) = backup.get("parameters").and_then(Value::as_mapping) {
            for (_, parameter) in parameters {
                paths.extend(strings(parameter.get("adds_paths")));
            }
        }
        recipes.insert(name.to_string(), paths);
    }
    recipes
}

/// Whether `path` sits within any of `granted` -- systemd grants a
/// `ReadWritePaths` entry recursively and rsync captures a Recipe path
/// recursively, both at a directory boundary only.
fn within(path: &str, granted: &[String]) -> bool {
    granted.iter().any(|grant| {
        let grant = grant.trim_end_matches('/');
        grant == path || path.starts_with(&format!("{grant}/"))
    })
}

/// What a directory is for, which decides its relationship to the Backup
/// Recipe: `Data` must sit within the Recipe's paths and everything else must
/// not. Writability is deliberately not derivable from the kind -- that is the
/// corrected reading of #624: five roles deny their unit directories of every
/// kind, and each denial is a per-unit judgement, so it is declared per
/// directory in `writers` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A deploy puts it back: the install tree, a rendered config.
    Install,
    /// Contents in flight or derived: drained, regenerated or republished by
    /// the next run, with the source of record elsewhere.
    Staging,
    /// The store of record -- what a restore must put back.
    Data,
    /// Operational state the fleet accepts losing: cursors, scratch, watch
    /// state.
    Expendable,
}

/// One service-owned directory, classified. The set of these is matched
/// against what the roles actually create, by equality in both directions.
struct DeclaredDirectory {
    role: &'static str,
    /// The path as the role's defaults resolve it.
    dir: &'static str,
    kind: Kind,
    /// Exactly the strict units able to write here: every strict unit of the
    /// role is asserted against this list in both directions, so a denial is
    /// declared least privilege and a new grant fails until classified.
    writers: &'static [&'static str],
    why: &'static str,
}

const DECLARED_DIRECTORIES: &[DeclaredDirectory] = &[
    DeclaredDirectory {
        role: "actual",
        dir: "/var/lib/actual",
        kind: Kind::Data,
        writers: &["actual.service"],
        why: "server accounts, the server password, Enable Banking credentials and budget \
              blobs -- the store the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "baikal",
        dir: "/opt/baikal",
        kind: Kind::Install,
        writers: &[],
        why: "the release tree a deploy puts back; what serves it is apt's php-fpm outside \
              this model, and the two sync oneshots are each granted only their own target",
    },
    DeclaredDirectory {
        role: "baikal",
        dir: "/opt/baikal/Specific",
        kind: Kind::Data,
        writers: &[],
        why: "the CalDAV store of record, written by apt's php-fpm, which no templated unit \
              confines; the narrow oneshots deliberately cannot reach it above db/",
    },
    DeclaredDirectory {
        role: "baikal",
        dir: "/opt/baikal/Specific/db",
        kind: Kind::Data,
        writers: &["baikal-birthday-sync.service", "baikal-busy-sync.service"],
        why: "the sqlite database both sync oneshots write into; the Recipe captures it \
              through its parent",
    },
    DeclaredDirectory {
        role: "baikal",
        dir: "/opt/baikal/busy",
        kind: Kind::Staging,
        writers: &["baikal-busy-sync.service"],
        why: "the derived Busy Feed, republished on every firing; Baikal's calendars are \
              the source of record",
    },
    DeclaredDirectory {
        role: "baikal",
        dir: "/opt/baikal/config",
        kind: Kind::Install,
        writers: &[],
        why: "ansible renders baikal.yaml into it; a deploy puts it back",
    },
    DeclaredDirectory {
        role: "bichon",
        dir: "/opt/bichon",
        kind: Kind::Install,
        writers: &[],
        why: "holds the binary and the encrypt password; a deploy puts both back, and the \
              server is granted only its data/ subtree",
    },
    DeclaredDirectory {
        role: "bichon",
        dir: "/opt/bichon/data",
        kind: Kind::Data,
        writers: &["bichon.service"],
        why: "the Internal Store root the Recipe rsyncs since its account registry proved \
              original state (ADR-0031)",
    },
    DeclaredDirectory {
        role: "bichon",
        dir: "/opt/bichon/data/index",
        kind: Kind::Data,
        writers: &["bichon.service"],
        why: "the search index -- derived, but inside the Internal Store the Recipe rsyncs \
              wholesale; carving it out would cost more than it saves",
    },
    DeclaredDirectory {
        role: "bichon",
        dir: "/opt/bichon/data/store",
        kind: Kind::Data,
        writers: &["bichon.service"],
        why: "the message store inside the Internal Store root",
    },
    DeclaredDirectory {
        role: "bichon",
        dir: "/var/lib/bichon-archive",
        kind: Kind::Data,
        writers: &["bichon-archive.service"],
        why: "the Email Archive, the Bichon-independent mirror the Recipe rsyncs alongside \
              the Internal Store (ADR-0006, ADR-0031)",
    },
    DeclaredDirectory {
        role: "bichon",
        dir: "/var/lib/bichon-archive/.state",
        kind: Kind::Data,
        writers: &["bichon-archive.service"],
        why: "the Archive Cursors -- inside the archive tree the Recipe rsyncs, so they \
              restore with what they index",
    },
    DeclaredDirectory {
        role: "bichon",
        dir: "/var/lib/bichon-uidvalidity-watch",
        kind: Kind::Expendable,
        writers: &["bichon-uidvalidity-watch.service"],
        why: "the Rebuild Latch's own state, outside every backed-up path by design; only \
              the watch unit writes it, which is exactly why the timer stays out of the \
              Recipe's quiesce order (ADR-0032)",
    },
    DeclaredDirectory {
        role: "calibre",
        dir: "/home/calibre",
        kind: Kind::Data,
        writers: &[],
        why: "calibre-web keeps its config under the service home; the unit is not \
              ProtectSystem=strict, so no writability fact exists to assert",
    },
    DeclaredDirectory {
        role: "calibre",
        dir: "/opt/calibre",
        kind: Kind::Data,
        writers: &[],
        why: "an unpinned pip venv (`state: present`) no deploy reproduces byte-for-byte; \
              the Recipe captures it whole rather than betting on what upstream still \
              serves",
    },
    DeclaredDirectory {
        role: "calibre",
        dir: "/srv/calibre",
        kind: Kind::Data,
        writers: &[],
        why: "the library: books and metadata.db",
    },
    DeclaredDirectory {
        role: "colporteur",
        dir: "/opt/colporteur",
        kind: Kind::Install,
        writers: &[],
        why: "holds the binary; the unit execs it afresh at every firing and a deploy puts \
              it back -- the service cannot rewrite its own executable",
    },
    DeclaredDirectory {
        role: "colporteur",
        dir: "/var/lib/colporteur",
        kind: Kind::Staging,
        writers: &["colporteur.service"],
        why: "holds only the feeds tree below; no Recipe exists because nothing here is a \
              store of record",
    },
    DeclaredDirectory {
        role: "colporteur",
        dir: "/var/lib/colporteur/feeds",
        kind: Kind::Staging,
        writers: &["colporteur.service"],
        why: "Atom XML derived from the Upstream Mailbox, which remains the source of \
              record; a lost feed is regenerated on the next firing",
    },
    DeclaredDirectory {
        role: "freshrss",
        dir: "/opt/freshrss",
        kind: Kind::Install,
        writers: &[],
        why: "the git checkout; the unit is denied its own install tree, data/ excepted",
    },
    DeclaredDirectory {
        role: "freshrss",
        dir: "/opt/freshrss/data",
        kind: Kind::Data,
        writers: &["freshrss.service"],
        why: "FreshRSS state inside the checkout -- upstream's layout, not this repo's; \
              the Recipe captures it where it lives",
    },
    DeclaredDirectory {
        role: "freshrss",
        dir: "/var/lib/freshrss",
        kind: Kind::Data,
        writers: &["freshrss.service"],
        why: "state outside the checkout, in the Recipe alongside data/",
    },
    DeclaredDirectory {
        role: "gokapi",
        dir: "/var/lib/gokapi",
        kind: Kind::Data,
        writers: &["gokapi.service"],
        why: "the whole state tree the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "gokapi",
        dir: "/var/lib/gokapi/config",
        kind: Kind::Data,
        writers: &["gokapi.service"],
        why: "inside the state tree the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "gokapi",
        dir: "/var/lib/gokapi/custom",
        kind: Kind::Data,
        writers: &["gokapi.service"],
        why: "inside the state tree the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "gokapi",
        dir: "/var/lib/gokapi/data",
        kind: Kind::Data,
        writers: &["gokapi.service"],
        why: "inside the state tree the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "grimmory",
        dir: "/opt/grimmory",
        kind: Kind::Install,
        writers: &[],
        why: "holds the jar and the rendered .env, which ansible writes and the service \
              only reads; denied like colporteur's, freshrss's and tgtg's install trees \
              -- the fleet-consistency decision #624 settled",
    },
    DeclaredDirectory {
        role: "grimmory",
        dir: "/srv/bookdrop",
        kind: Kind::Staging,
        writers: &["grimmory.service"],
        why: "a staging folder grimmory drains into the library; its contents are in \
              flight, not the store of record",
    },
    DeclaredDirectory {
        role: "grimmory",
        dir: "/srv/books",
        kind: Kind::Data,
        writers: &["grimmory.service"],
        why: "the library root, the one its own Path Attestation confirms against the \
              Recipe before every backup (ADR-0033)",
    },
    DeclaredDirectory {
        role: "grimmory",
        dir: "/srv/grimmory",
        kind: Kind::Data,
        writers: &["grimmory.service"],
        why: "grimmory's own data directory, in the Recipe",
    },
    DeclaredDirectory {
        role: "headscale",
        dir: "/var/lib/headscale",
        kind: Kind::Data,
        writers: &["headscale.service"],
        why: "the coordination database and noise keys the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "navidrome",
        dir: "/srv/music",
        kind: Kind::Data,
        writers: &[],
        why: "the music library, pushed behind --include-music (the Recipe's adds_paths); \
              apt's unit runs navidrome, so no strict unit exists to assert against",
    },
    DeclaredDirectory {
        role: "navidrome",
        dir: "/var/lib/navidrome",
        kind: Kind::Data,
        writers: &[],
        why: "navidrome's database and cache, in the Recipe; apt owns the unit",
    },
    DeclaredDirectory {
        role: "paperless",
        dir: "/opt/paperless",
        kind: Kind::Install,
        writers: &[
            "paperless-consumer.service",
            "paperless-scheduler.service",
            "paperless-task-queue.service",
            "paperless-webserver.service",
        ],
        why: "the source tree a deploy deletes and re-unpacks (#604); the grant covers the \
              whole tree so the nested data, media, consume and scratch stay reachable -- \
              narrowing it to the four subtrees is its own decision, not this fence's",
    },
    DeclaredDirectory {
        role: "paperless",
        dir: "/opt/paperless/consume",
        kind: Kind::Staging,
        writers: &[
            "paperless-consumer.service",
            "paperless-scheduler.service",
            "paperless-task-queue.service",
            "paperless-webserver.service",
        ],
        why: "the consumption dir: documents in flight into the archive, gone once \
              consumed",
    },
    DeclaredDirectory {
        role: "paperless",
        dir: "/opt/paperless/data",
        kind: Kind::Data,
        writers: &[
            "paperless-consumer.service",
            "paperless-scheduler.service",
            "paperless-task-queue.service",
            "paperless-webserver.service",
        ],
        why: "the index and classifier state the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "paperless",
        dir: "/opt/paperless/media",
        kind: Kind::Data,
        writers: &[
            "paperless-consumer.service",
            "paperless-scheduler.service",
            "paperless-task-queue.service",
            "paperless-webserver.service",
        ],
        why: "the originals and archived documents the Recipe rsyncs",
    },
    DeclaredDirectory {
        role: "paperless",
        dir: "/opt/paperless/scratch",
        kind: Kind::Expendable,
        writers: &[
            "paperless-consumer.service",
            "paperless-scheduler.service",
            "paperless-task-queue.service",
            "paperless-webserver.service",
        ],
        why: "per-run OCR scratch (#482); nothing outlives the task that wrote it",
    },
    DeclaredDirectory {
        role: "tgtg",
        dir: "/opt/tgtg",
        kind: Kind::Install,
        writers: &[],
        why: "the git checkout, explicitly ReadOnlyPaths to its own unit",
    },
    DeclaredDirectory {
        role: "tgtg",
        dir: "/opt/tgtg/.venv",
        kind: Kind::Install,
        writers: &[],
        why: "the venv a deploy rebuilds, inside the read-only install tree",
    },
    DeclaredDirectory {
        role: "tgtg",
        dir: "/var/lib/tgtg",
        kind: Kind::Data,
        writers: &["tgtg.service"],
        why: "tokens and state the Recipe rsyncs -- losing them means re-pairing with the \
              vendor app",
    },
    DeclaredDirectory {
        role: "yourls",
        dir: "/var/www/yourls",
        kind: Kind::Data,
        writers: &[],
        why: "the checkout carries its config and plugins and is served and written by \
              apt's php-fpm, which no role-templated unit confines",
    },
];

/// A Recipe path no directory task creates for a service user -- the Recipe
/// side's own declared remainder, so guard (ii)'s reverse direction can be
/// equality instead of a subset that rots.
struct RecipeOnlyPath {
    app: &'static str,
    path: &'static str,
    why: &'static str,
}

const RECIPE_ONLY_PATHS: &[RecipeOnlyPath] = &[
    RecipeOnlyPath {
        app: "navidrome",
        path: "/etc/navidrome",
        why: "the deb's config dir; the role renders navidrome.toml into it but creates \
              no directory for a service user there",
    },
    RecipeOnlyPath {
        app: "syncthing",
        path: "/home/{admin_user}/.local/state/syncthing/config.xml",
        why: "a file under the admin home (ADR-0023), not a service-owned directory",
    },
    RecipeOnlyPath {
        app: "syncthing",
        path: "/home/{admin_user}/.local/state/syncthing/cert.pem",
        why: "a file under the admin home (ADR-0023), not a service-owned directory",
    },
    RecipeOnlyPath {
        app: "syncthing",
        path: "/home/{admin_user}/.local/state/syncthing/key.pem",
        why: "a file under the admin home (ADR-0023), not a service-owned directory",
    },
];

/// The roles this fence scans: everything with a strict unit (guard (i) has a
/// writability fact there) or a Backup Recipe (guard (ii) has a coverage fact
/// there). Computed, not listed -- a new strict unit or a new Recipe pulls its
/// role in on its own (ADR-0028's lesson: enumerating a blind spot undercounts
/// by exactly what you cannot see).
fn scanned_roles() -> Vec<(String, Vec<StrictUnit>, BTreeSet<String>)> {
    let recipes = recipes();
    let installed = fleet_units();
    let mut domain = Vec::new();
    for role in all_roles() {
        let vars = defaults(&role);
        let units = strict_units(&installed, &role, &vars);
        if units.is_empty() && !recipes.contains_key(&role) {
            continue;
        }
        let users = service_users(&role, &vars, &units);
        let dirs = created_directories(&role, &vars, &users);
        domain.push((role, units, dirs));
    }
    domain
}

/// Both directions of the classification's coverage: a directory a role
/// creates for a service user that no declaration names fails the build until
/// a human classifies it, and a declaration naming a directory no role
/// creates fails until it is removed.
#[test]
fn test_every_service_owned_directory_is_classified() {
    let computed: BTreeSet<(String, String)> = scanned_roles()
        .into_iter()
        .flat_map(|(role, _, dirs)| dirs.into_iter().map(move |dir| (role.clone(), dir)))
        .collect();
    let declared: BTreeSet<(String, String)> = DECLARED_DIRECTORIES
        .iter()
        .map(|declaration| (declaration.role.to_string(), declaration.dir.to_string()))
        .collect();

    let unclassified: Vec<String> = computed
        .difference(&declared)
        .map(|(role, dir)| format!("  {role}: {dir}"))
        .collect();
    let stale: Vec<String> = declared
        .difference(&computed)
        .map(|(role, dir)| format!("  {role}: {dir}"))
        .collect();
    assert!(
        unclassified.is_empty() && stale.is_empty(),
        "every directory a role creates for a service user carries a declared \
         classification (#624)\nunclassified -- add a DeclaredDirectory:\n{}\nstale -- the \
         role no longer creates these, remove the declaration:\n{}",
        unclassified.join("\n"),
        stale.join("\n")
    );
}

/// Guard (i), fleet-wide and exact: for every strict unit of a role and every
/// classified directory, the unit can write there iff the declaration names
/// it a writer. The 13 denials the #624 scan found are all deliberate least
/// privilege -- this is where each one is declared to be, and where a
/// loosened or tightened `ReadWritePaths` fails until the declaration moves
/// with it.
#[test]
fn test_write_access_matches_the_declared_writers() {
    for (role, units, _) in scanned_roles() {
        let unit_names: BTreeSet<&str> = units.iter().map(|unit| unit.name.as_str()).collect();
        for declaration in DECLARED_DIRECTORIES
            .iter()
            .filter(|declaration| declaration.role == role)
        {
            for writer in declaration.writers {
                assert!(
                    unit_names.contains(writer),
                    "{role}: {} declares `{writer}` a writer, but the role templates no \
                     strict unit of that name",
                    declaration.dir
                );
            }
            for unit in &units {
                let can = within(declaration.dir, &unit.grants);
                let should = declaration.writers.contains(&unit.name.as_str());
                assert!(
                    !can || should,
                    "{role}: `{}` can write {} but the classification says it must not -- \
                     {}; if the new grant is deliberate, move the declaration with it",
                    unit.name,
                    declaration.dir,
                    declaration.why
                );
                assert!(
                    can || !should,
                    "{role}: `{}` is declared a writer of {} but the unit cannot write \
                     there; under ProtectSystem=strict that is one EROFS away from a \
                     failed run (#621) -- {}",
                    unit.name,
                    declaration.dir,
                    declaration.why
                );
            }
        }
    }
}

/// Guard (ii), fleet-wide: the Backup Recipe captures a directory iff its
/// kind is Data. Both failure modes of #621 stay fenced -- a data directory
/// the Recipe misses restores to metadata pointing at nothing, and a Recipe
/// entry nothing classifies as data is either stale or an unclassified
/// store of record. Containment, not equality: gokapi's one Recipe path
/// covers three nested directories, and that is the Recipe's semantics
/// (rsync is recursive), not a drift.
#[test]
fn test_a_recipe_captures_a_directory_iff_its_kind_is_data() {
    let recipes = recipes();
    let empty = Vec::new();
    for declaration in DECLARED_DIRECTORIES {
        let paths = recipes.get(declaration.role).unwrap_or(&empty);
        let covered = within(declaration.dir, paths);
        let data = declaration.kind == Kind::Data;
        assert!(
            !data || covered,
            "{}: {} is classified Data but no Recipe path covers it; the Recipe is the \
             sole record of what a restore puts back (#621) -- {}",
            declaration.role,
            declaration.dir,
            declaration.why
        );
        assert!(
            data || !covered,
            "{}: the Recipe captures {} but its classification says {:?} -- one of them \
             is wrong; {}",
            declaration.role,
            declaration.dir,
            declaration.kind,
            declaration.why
        );
    }
}

/// The reverse of guard (ii): every path a Recipe declares -- parameter-gated
/// ones included -- is either a directory some role creates and classifies as
/// Data, or is declared Recipe-only with its reason. Removing a `paths:`
/// entry from a meta file breaks the Data directory it covered (above);
/// adding one nothing accounts for breaks here.
#[test]
fn test_every_recipe_path_is_classified_data_or_declared_recipe_only() {
    let recipes = recipes();
    let data_dirs: BTreeSet<(&str, &str)> = DECLARED_DIRECTORIES
        .iter()
        .filter(|declaration| declaration.kind == Kind::Data)
        .map(|declaration| (declaration.role, declaration.dir))
        .collect();
    let extras: BTreeSet<(&str, &str)> = RECIPE_ONLY_PATHS
        .iter()
        .map(|extra| (extra.app, extra.path))
        .collect();

    for (app, paths) in &recipes {
        for path in paths {
            assert!(
                data_dirs.contains(&(app.as_str(), path.as_str()))
                    || extras.contains(&(app.as_str(), path.as_str())),
                "{app}: the Recipe pushes {path}, but no role creates it as a classified \
                 Data directory and no RecipeOnlyPath vouches for it -- classify what it \
                 is before trusting a restore to put it back (#624)"
            );
        }
    }

    for extra in RECIPE_ONLY_PATHS {
        assert!(
            recipes
                .get(extra.app)
                .is_some_and(|paths| paths.iter().any(|path| path == extra.path)),
            "{}: {} is declared Recipe-only because it {}, but the Recipe no longer \
             lists it -- remove the declaration",
            extra.app,
            extra.path,
            extra.why
        );
    }
}
