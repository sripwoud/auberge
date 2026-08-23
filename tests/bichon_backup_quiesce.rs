use std::fs;
use std::path::PathBuf;

/// Bichon's Backup Recipe stops `bichon-archive.timer` ahead of `bichon`
/// (#619). That order is only worth anything because of two edges declared in
/// the role's unit templates, and neither is asserted anywhere else:
///
/// - the timer triggers `bichon-archive.service`, so leaving it active during
///   the window lets an hourly tick start the archive mid-rsync;
/// - `bichon-archive.service` **requires** `bichon.service`, so that tick pulls
///   the deliberately-stopped server back up over a live copy of its Internal
///   Store, and — the other half — stopping `bichon` propagates down to an
///   archive run already in flight.
///
/// Drop the `Requires=` line and #619 comes back silently: the recipe still
/// stops both units, the tests still pass, and nothing tears until a tick lands
/// inside a nightly window. The sibling `bichon-uidvalidity-watch.service`
/// deliberately omits the same line, so its absence here would read as
/// intentional rather than as a regression.
fn templates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles/bichon/templates")
}

fn read_template(name: &str) -> String {
    let path = templates_dir().join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()))
}

fn declares(template: &str, directive: &str) -> bool {
    template
        .lines()
        .map(str::trim)
        .any(|line| line == directive)
}

#[test]
fn archive_service_requires_the_server_it_calls() {
    assert!(
        declares(
            &read_template("bichon-archive.service.j2"),
            "Requires=bichon.service"
        ),
        "bichon-archive.service must require bichon.service — the recipe's quiesce order (#619) \
         exists because this edge lets an archive tick restart the server mid-backup"
    );
}

#[test]
fn archive_service_is_ordered_after_the_server() {
    assert!(
        declares(
            &read_template("bichon-archive.service.j2"),
            "After=bichon.service"
        ),
        "bichon-archive.service must be ordered after bichon.service, so stopping the server \
         takes an in-flight archive run down first"
    );
}

#[test]
fn archive_timer_triggers_the_archive_service() {
    assert!(
        declares(
            &read_template("bichon-archive.timer.j2"),
            "Unit=bichon-archive.service"
        ),
        "bichon-archive.timer must trigger bichon-archive.service — the recipe stops this timer \
         to quiesce that trigger (#619)"
    );
}

/// The watch is deliberately outside the Recipe: it reads only the journal and
/// writes only its own state dir, so it has no edge into the backed-up paths.
/// A `Requires=bichon.service` here would give it one, and the Recipe would
/// have to quiesce it too.
#[test]
fn uidvalidity_watch_stays_independent_of_the_server() {
    assert!(
        !declares(
            &read_template("bichon-uidvalidity-watch.service.j2"),
            "Requires=bichon.service"
        ),
        "bichon-uidvalidity-watch.service must not require bichon.service — it is left out of \
         the Backup Recipe on the strength of having no edge into the server"
    );
}
