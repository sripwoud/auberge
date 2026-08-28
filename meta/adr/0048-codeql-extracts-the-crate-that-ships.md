# ADR-0048: CodeQL extracts the crate that ships, not the test binary

## Status

Accepted, 2026-08-28.

## Decision

**The Rust CodeQL database is built from the crate as it ships.** `.github/workflows/codeql.yml` sets `CODEQL_EXTRACTOR_RUST_OPTION_CARGO_CFG_OVERRIDES: "-test"`, so `#[cfg(test)] mod tests` is not compiled into the database and a literal that exists only to drive a unit test is not a dataflow source.

The extractor makes this opt-out. `to_cfg_overrides` in `rust/extractor/src/config.rs` starts with `enabled_cfgs.insert(to_cfg_override("test"))` and only then reads the option, where a `-` prefix moves an atom to the disabled set.

Three details make the one line work, and each was read out of the extractor rather than assumed, because an unparsed extractor option looks exactly like a working one:

- **The variable is the extractor's own config channel, not a side door.** `Config::extract` merges `Env::prefixed("CODEQL_EXTRACTOR_RUST_OPTION_")` — the same place `codeql database create --extractor-option cargo_cfg_overrides=…` deposits its value. `codeql-action/init@v4` exposes no extractor-option input, so from a workflow the variable is the only route, and it is the route the action itself takes: `runExtraction` sets `CODEQL_EXTRACTOR_JAVA_OPTION_BUILDLESS_DEPENDENCY_DIR` and its C# counterpart in exactly this shape.
- **A bare `-test` is the encoding.** The field is `Vec<String>`, deserialized by `deserialize_newline_or_comma_separated_vec`, whose `visit_str` splits a lone string on `,` and `\n`. No bracket syntax; upstream's `test_cfg_overrides_option.py` passes exactly this comma-joined form.
- **The CLI defaults cannot clobber it.** `Serialized::defaults(cli_args)` is merged _after_ both env providers and would win on a key it carried, except the generated `CliConfig` maps every `Vec` field to `Option<String>` under `skip_serializing_none`, so an unpassed flag contributes no key at all.

It sits at job level rather than on a step because extraction under `build-mode: none` happens in `analyze`, not `init` — `runFinalize` calls `runExtraction` before `finalizeDatabase`. It is inert on the `actions` and `python` legs: they never spawn the Rust extractor, and no other extractor reads this variable. Carrying it on the rust matrix entry instead would scope it to the leg #696 names, and is rejected twice over: `${{ matrix.… }}` resolves to the empty string on the legs that omit the key, and an empty `CARGO_CFG_OVERRIDES` does not mean “no override” — it splits to one element and enables an anonymous atom; and job level keeps holding if codeql-action ever moves extraction back into `init`.

### What it costs

**Inline unit tests stop being scanned, and unscanned reads as clean.** Rust unit tests live in `src/*.rs` beside the code they test, so this is not an exclusion a reviewer can see as a path in a config. Accepted: test code is not shipped, and every alert it has produced here has been noise. The cost is the one ADR-0039 names — a quiet check reading as coverage — and it is paid knowingly on a surface that ships nothing.

**Nothing in the repo guards it.** The failure mode is loud in one direction and silent in the other. If the variable is dropped or misspelled, the test-sourced findings come back and are visible. If `-test` were to take integration harnesses down with it, CodeQL would simply get quieter, and there is no in-repo fence that could tell — the property lives in the database, not the tree. That half is settled by measurement below rather than by argument, and re-measuring is the check if the extractor's cfg handling ever changes.

**It does not close the `src/config.rs` finding** (alert #25, `rust/hard-coded-cryptographic-value`). That source is production: `"auberge/config.toml"` at `src/config.rs:37` taints `Config::path()`, and the engine loses field precision through `Config::load`, so the literal reaches `restic::command`'s `RESTIC_PASSWORD` env. A query-engine limitation, not a repo defect; dismissal remains the only handling.

## Why

Six results survive ADR-0011's `rust/cleartext-logging` exclusion on master. Five of them exist only because the test binary was extracted, and they fail in two different ways:

| Rule                                     | Reported at                                    | Why it is noise                                                                                                                             |
| ---------------------------------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `rust/non-https-url` ×3                  | `src/commands/versions.rs:307`, `:326`, `:346` | Production lines. The taint source is `"http://unused"` at `versions.rs:825-827`, inside the test module.                                   |
| `rust/hard-coded-cryptographic-value` ×2 | `src/services/backup/restic.rs:216`, `:237`    | The reported line _is_ the fixture — the passwords handed to `command` and `ls_command`, `"s3cret"` and `""`. Neither carries a `codeFlow`. |

The second kind is noise. The first kind is a trap.

`#[cfg(test)] mod tests` starts at `src/commands/versions.rs:518`. The flow CodeQL reported for the first of them:

```
src/commands/versions.rs:827  "http://unused"
  -> versions.rs:277  with_bases(..)
  -> versions.rs:282  Self {...}          [UpstreamClient.go_base]
  -> versions.rs:304  self.go_base
  -> versions.rs:307  &url                <- alert lands here
```

Production never takes that path: `UpstreamClient::new` passes the three `https://` constants at `versions.rs:12-14`. The reader who opens `versions.rs:307` finds nothing wrong there, because nothing is.

That is ADR-0011's dismissal toil arriving through a different door, and it is worse than the one ADR-0011 closed. A `cleartext-logging` hit at least pointed at the line it meant. These point at a line whose behaviour is correct, and the only way to see that is to read the SARIF's `codeFlows` — the REST alert body shows the reported location and a one-line message, and neither of them mentions the test.

### The alert list could not have shown the fix working

All six were dismissed by hand during triage, before any fix existed. A dismissed alert stays dismissed and keeps appearing in later analyses, so an open-alert count reads identically whether the extraction changed or not: #696's “alerts #26/#27/#28 close on the next scan” is no longer a test that can be run, and the SARIF stands in for it deliberately rather than by convenience. The instrument is the analysis SARIF's `results`, which is engine output: a result that leaves it is a source the extractor stopped seeing.

### Measured

Two databases were built with CodeQL 2.26.4 from the tree at `a75c823` plus this change — one with the variable unset, one with it set exactly as the workflow sets it — and analysed with the same `rust-code-scanning` suite. The counts below are that tree's; they move whenever `src/` does, and the invariant they establish is the row that does not move. The baseline reproduces CI's six results exactly once `rust/cleartext-logging` is removed (23 results, 17 of them that query), so the pair is comparable to what the Actions job produces.

The extractor echoes its parsed configuration into the CLI's build output, which makes the parse legible on every run rather than only in this experiment:

```
cargo_cfg_overrides: [],          # variable unset
cargo_cfg_overrides: ["-test"],   # variable set
```

|                                               | baseline | `-test` |
| --------------------------------------------- | -------: | ------: |
| results, suite minus `rust/cleartext-logging` |        6 |   **1** |
| results, whole default suite                  |       23 |      18 |
| functions extracted from `src/**`             |     2385 |    1325 |
| functions extracted from `tests/**`           |      391 | **391** |
| files in the database                         |       84 |      84 |

The survivor is `src/config.rs:37`. The five that leave are the five above.

`tests/**` is the number that had to be measured rather than argued, because `-test` is a _global_ override and the harnesses there are compiled as test targets. Their 391 functions are identical file by file, and the database's file set does not change: the override drops `#[cfg(test)]` items, not test targets. The 17 `rust/cleartext-logging` results are unchanged too — a second witness from the other side, since those sit in `src/output.rs` and `src/commands/bichon/*`.

What `src/**` loses is 1060 functions, across 45 of the 53 `src/` files that database holds: the inline test bodies plus the fixtures and helpers beside them. No file leaves the database — the four `src/` rows sitting at zero functions are module declarations, and they read zero in both.

## Alternatives considered

- **`paths-ignore` the test code.** Rejected: there is no file to exclude. Rust unit tests live inline in `src/*.rs`, so the only path that covers them is `src/**`, which would take every query off the entire CLI surface — the blast-radius arithmetic on which ADR-0011 rejected its own `paths-ignore` option. Its _other_ rejection, of per-path query suppression, does not transfer: that failed because `query-filters` carry no `paths` key, and its paths were perfectly nameable.
- **Exclude `rust/non-https-url` and `rust/hard-coded-cryptographic-value` repo-wide,** as ADR-0011 excluded `rust/cleartext-logging`. Rejected, and the distinction matters: ADR-0011's query does not apply to this repo at all, because auberge has no log sink. These two do apply. A genuine hard-coded credential, or a genuine `http://` upstream, is exactly what should be caught. The query is right and its input was wrong; correcting the input keeps the query.
- **Move the unit tests out of `src/`.** ADR-0046 gave the crate a `src/lib.rs`, so some unit tests could now live under `tests/`. Rejected: private items stay unreachable, so the move either drops those tests or widens visibility for the analyser's benefit — changing what the crate exports to quiet a scanner.
- **`--extractor-option` through `CODEQL_ACTION_EXTRA_OPTIONS`.** codeql-action forwards extra CLI flags to `database init` from that variable. Rejected: it is an undocumented escape hatch, it would have to name the subcommand that actually extracts, and the CLI's only way to hand the value to the extractor is the environment variable this ADR sets directly.
- **Keep dismissing by hand.** The status quo, and what ADR-0011 exists to end.
