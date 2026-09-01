//! #780's slice 3 (#786): the generated ssh include cannot go stale, because
//! regenerating it is bound to the roster write rather than remembered by
//! each command.
//!
//! The defect this closes: `hosts.toml` had four mutation paths that called
//! `sync_ssh_include()` and one — `host detect-tailscale-ip`, the command
//! that will start moving addresses under #787 — that did not. A stale
//! include means interactive `ssh <name>` and the CLI take different routes
//! to the same Host, which is the ten-day outage #780 exists to make
//! structurally impossible, not merely discouraged.
//!
//! A sibling to `one_route_to_a_host.rs`, inheriting the module walk from
//! `crate_source` (#679): that one fences where an address comes *from*, this
//! fences whether the file that publishes it was rewritten. Two directions,
//! because the binding fails two ways — something regenerates the include
//! outside the writer (the "remember to call it" pattern coming back), or
//! something inside `hosts.rs` writes the roster without going through the
//! writer at all.
//!
//! The third direction — the writer stops regenerating — is a behavioural
//! question, not a textual one, and is asserted where it can be observed:
//! `hosts::tests::write_roster_regenerates_the_ssh_include_from_the_same_hosts`
//! writes both files into temp directories and compares them.

mod crate_source;

use crate_source::{code_lines, find, modules, names_in_code};

/// The one module that regenerates `~/.ssh/config.d/auberge.conf`, because it
/// is the one that writes `hosts.toml`.
const ROSTER_WRITER: &str = "src/hosts.rs";

/// Where the regeneration functions are defined. Naming them here is not
/// calling them.
const INCLUDE_MODULE: &str = "src/services/ssh_include.rs";

/// Regenerating the include is the roster writer's job alone. A call to
/// either of these anywhere else is a command remembering to do it, which is
/// the design that produced the omission.
const SYNC_CALL: &str = "ssh_include::sync(";
const REGENERATION_CALLS: &[&str] = &[SYNC_CALL, "write_include_file("];

/// The name of the one function in [`ROSTER_WRITER`] allowed to write the
/// roster file — the same function that regenerates the include.
const ROSTER_WRITE_FN: &str = "fn write_roster(";

/// The module that renders the include, and the function inside it whose
/// output ssh actually obeys.
const INCLUDE_RENDERER: (&str, &str) = ("src/services/ssh_include.rs", "pub fn render(");

/// Declaration fields that must not reach a connection directive.
///
/// `host.name` is deliberately absent: it is the stanza's own keyword — the
/// alias a human types as `ssh <name>` — which is a declaration, not a route.
/// `host.address` is absent because `one_route_to_a_host.rs` already forbids
/// it across this whole module; duplicating it here would give one rule two
/// homes that can drift apart.
const DECLARATION_READS: &[&str] = &["host.port", "host.user"];

/// What the directives must be read from instead. Asserted as well as the ban
/// above, because a `render` that stopped emitting `Port` and `User` entirely
/// would satisfy the ban and publish an unusable stanza.
const ROUTE_READS: &[&str] = &["route.address", "route.port", "route.user"];

/// The body of the top-level `fn` whose declaration contains `decl`, from the
/// declaration to the first column-0 `}`.
///
/// Scoped to the function rather than the module because `identity_file`
/// legitimately reads `host.user`: the default key path
/// `~/.ssh/identities/<name>/<user>` is a local filesystem convention keyed on
/// the declaration, and no routing policy may move it. A module-wide ban would
/// have to exempt that read, and an exemption broad enough to cover it is
/// broad enough to cover the directives too.
fn fn_body<'a>(source: &'a str, decl: &str) -> &'a str {
    let start = source
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} must exist to be checked"));
    let rest = &source[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("{decl} must have a column-0 closing brace"));
    &rest[..end]
}

/// The module's source with its trailing `#[cfg(test)] mod tests` cut off.
///
/// `hosts.rs`'s own tests legitimately write files — one of them plants a
/// regular file where `config.d` must be created, to prove a failed
/// regeneration stops the command — and counting those as roster writes would
/// make the fence below fire on the test that keeps it honest.
///
/// The cut is anchored on the test *module*, not on a bare `#[cfg(test)]`:
/// `hosts.rs` also carries a `#[cfg(test)]` fixture constructor just above
/// `impl HostManager`, and cutting at the first `#[cfg(test)]` would hide the
/// roster writer itself — a fence passing over a domain it can no longer see. If the anchor
/// ever stops matching, the whole file is scanned, which fails loudly rather
/// than passing quietly.
fn without_tests(source: &str) -> &str {
    match source.find(TEST_MODULE) {
        Some(at) => &source[..at],
        None => source,
    }
}

const TEST_MODULE: &str = "#[cfg(test)]\nmod tests {";

#[test]
fn only_the_roster_writer_regenerates_the_include() {
    let walked = modules();
    let mut offenders: Vec<String> = Vec::new();

    for module in &walked {
        if module.repo_relative == ROSTER_WRITER || module.repo_relative == INCLUDE_MODULE {
            continue;
        }
        for call in REGENERATION_CALLS {
            if names_in_code(&module.source, call) {
                offenders.push(format!(
                    "  {} calls {call} — regenerating the ssh include is bound to \
                     the roster write in {ROSTER_WRITER}, so no command has to \
                     remember it (#786)",
                    module.repo_relative
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "the ssh include is regenerated outside the roster write:\n{}",
        offenders.join("\n")
    );
}

/// Without this, deleting the binding would leave the scan above passing over
/// a crate where nothing regenerates the include at all.
#[test]
fn the_roster_writer_still_regenerates_the_include() {
    let walked = modules();
    let writer = find(&walked, ROSTER_WRITER);

    assert!(
        names_in_code(without_tests(&writer.source), SYNC_CALL),
        "{ROSTER_WRITER} no longer regenerates the ssh include — either the \
         binding moved (update ROSTER_WRITER) or every mutation path is now \
         free to leave ~/.ssh/config.d/auberge.conf stale, which is the \
         failure #786 closed"
    );
}

/// The roster is written once, by the function that also regenerates the
/// include. A second writer in the same module would be a mutation path that
/// bypasses the binding without ever leaving `hosts.rs` — invisible to the
/// scan above, which only watches the rest of the crate.
#[test]
fn the_roster_is_written_from_one_place() {
    let walked = modules();
    let source = without_tests(&find(&walked, ROSTER_WRITER).source);

    let writes: Vec<usize> = code_lines(source)
        .filter(|(_, line)| line.contains("fs::write("))
        .map(|(index, _)| index)
        .collect();

    assert_eq!(
        writes.len(),
        1,
        "{ROSTER_WRITER} writes a file from {} places; the roster must be \
         written only by {ROSTER_WRITE_FN}, which regenerates the ssh include \
         from the same slice (#786). Offending lines: {:?}",
        writes.len(),
        writes.iter().map(|i| i + 1).collect::<Vec<_>>()
    );

    let enclosing = enclosing_fn(source, writes[0]);
    assert!(
        enclosing.is_some_and(|decl| decl.contains(ROSTER_WRITE_FN)),
        "{ROSTER_WRITER}:{} writes a file from `{}` rather than \
         {ROSTER_WRITE_FN} — the roster write and the include regeneration \
         must stay in one function",
        writes[0] + 1,
        enclosing.unwrap_or("<no enclosing fn>")
    );
}

/// The nearest function declaration at or above `line`, trimmed — enough to
/// place a write inside a function without parsing Rust, and enough to name
/// the offending function when the write is in the wrong one.
fn enclosing_fn(source: &str, line: usize) -> Option<&str> {
    let above: Vec<&str> = source.lines().take(line + 1).collect();
    above
        .iter()
        .rev()
        .find(|l| l.trim_start().starts_with("fn ") || l.trim_start().starts_with("pub fn "))
        .map(|l| l.trim())
}

/// The stanza's `HostName`, `Port` and `User` are where ssh actually goes, so
/// they must come off the resolved `Route` — the same one the CLI's own ssh,
/// scp, rsync and ansible connections take. Reading them off the `Host`
/// beside it is what let interactive `ssh <name>` and `auberge` diverge
/// (#780); under #787 it would publish a tailnet address with the public
/// host's port.
///
/// `IdentityFile` is not covered, and must not be: it answers "what to write",
/// in the `~`-form ssh expands itself, where `Route::key_path` answers "what to
/// open" (ADR-0067).
#[test]
fn the_includes_connection_directives_come_off_the_route() {
    let (path, decl) = INCLUDE_RENDERER;
    let walked = modules();
    let body = fn_body(&find(&walked, path).source, decl);

    for read in DECLARATION_READS {
        assert!(
            !names_in_code(body, read),
            "{path}'s `{decl}` reads `{read}` — every connection directive it \
             emits must come off the resolved Route, or the include and the \
             CLI can take different routes to one Host (#786)"
        );
    }

    for read in ROUTE_READS {
        assert!(
            names_in_code(body, read),
            "{path}'s `{decl}` no longer reads `{read}` — either it stopped \
             emitting that directive, or it went back to the Host declaration, \
             which would leave the ban above passing vacuously"
        );
    }
}
