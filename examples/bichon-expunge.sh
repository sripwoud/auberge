#!/usr/bin/env bash
#
# examples/bichon-expunge.sh
#
# REFERENCE SCRIPT — NOT SHIPPED IN THE auberge BINARY
# ---------------------------------------------------
# Expunges mail older than a window from an Upstream Mailbox, but only after
# proving the messages are in the Email Archive and in the off-host restic
# backup. Five gates run in order; all must pass:
#
#   1. off-host backup    `auberge backup verify --app bichon` exits 0
#   2. archive freshness  last bichon-archive.service run succeeded, recently
#   3. folder coverage    every in-window store message is archived, proven
#                         by Message-ID (auberge bichon verify-coverage), and
#                         distinct archived Message-IDs >= IMAP count
#   4. summary            exact commands, message count, snapshot evidence
#   5. typed confirmation operator types the folder name on a TTY
#
# Before any gate runs, --folder must be a Synced Folder: the Email Archive
# can vouch for nothing else.
#
# With --sweep the same gates cover every eligible (account, Synced Folder)
# pair on the Host in one run — an Expunge Sweep (CONTEXT.md, ADR-0007
# amendment 2026-08-03). Host-scoped gates (1, 2) run once; coverage (3) runs
# per pair, and a failing pair is skipped as a named finding instead of
# aborting the rest; gates 4-5 become one summary table and two typed
# checkpoints — the Bichon Host name (scope), then the grand message total
# (magnitude). The folder-name checksum has nothing to defend in a sweep: no
# folder is typed, the target set comes from the Synced Folder set the
# operator curates via Account Reconcile.
#
# A repeated --account narrows a sweep to those accounts, every Synced Folder
# of each. The folder set still comes from Account Reconcile, so the two
# checkpoints keep defending what they always defended and only the scope is
# smaller. A named account himalaya does not know aborts before any gate runs,
# so it cannot reach the report as a missing row the operator has to notice.
#
# Safety contract (ADR-0007) — load-bearing, do not "improve" away:
#   - stdin must be a TTY; a non-TTY run refuses the expunge unconditionally
#   - there is no --yes / --force, so no unattended expunge path exists
#   - the typed folder name is an intent checksum against a mangled --folder;
#     a sweep types the Host name and the grand total instead, and a bare y/N
#     is never accepted anywhere
#
# Designed for MXroute; works with any IMAP provider himalaya supports.
#
# Prerequisites:
#   - auberge with the `backup verify`, `bichon reconcile-folders`, and
#     `bichon verify-coverage` subcommands (>= the release carrying all
#     three), and `bichon_api_token` set in its config: the script asks
#     auberge for the Synced Folder set, which is the expunge eligibility
#     rule
#   - himalaya  (https://github.com/pimalaya/himalaya — Rust, project ethos),
#     configured, with an account named after the mailbox email address:
#     bichon keys the Email Archive by email (see sanitize_email in
#     ansible/roles/bichon/templates/bichon-archive.sh.j2), and this script
#     passes one value to both himalaya --account and the archive path
#   - jq
#   - key-based ssh access to the Bichon Host, with journal/systemctl read
#   - that ssh user in the `bichon` group: the Email Archive is 0750
#     bichon:bichon and gate 3 counts files inside it without sudo
#
# Exit codes:
#   0 — expunged, or (non-interactive) all gates passed and expunge skipped;
#       for a sweep: completed with no finding
#   1 — a gate failed, a typed confirmation or checkpoint did not match, or a
#       sweep completed with at least one finding
#   2 — usage error, missing prerequisite, unreachable Host, or a sweep whose
#       target set is empty
#
# shellcheck shell=bash

set -euo pipefail

readonly PROGRAM_NAME="${0##*/}"
readonly DEFAULT_FOLDER='INBOX'
readonly DEFAULT_WINDOW_DAYS=90
readonly DEFAULT_ARCHIVE_PATH='/var/lib/bichon-archive'
# bichon-archive.timer is OnCalendar=hourly with RandomizedDelaySec=10min, so a
# healthy Host runs it at most ~70min apart. 3h leaves slack for a reboot.
readonly ARCHIVE_MAX_AGE_HOURS=3
# himalaya pages envelope listings; one oversized page beats paginating here.
readonly PAGE_SIZE=9999
readonly SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10)

host=''
folder=''

# Every --account, in flag order. It accumulates rather than overwrites so a
# sweep can be narrowed to some accounts; a single target is the one-element
# case, and two of them without --sweep is contradictory intent.
requested_accounts=()

# The one account every gate reads. Resolved from requested_accounts or the
# menu for a single target; reassigned per account as the sweep walks.
account=''
window_days="${DEFAULT_WINDOW_DAYS}"
archive_path="${DEFAULT_ARCHIVE_PATH}"
no_input=false
sweep=false

# Captured by check_tools, reused by the --account menu so himalaya is only
# asked once.
HIMALAYA_ACCOUNTS_JSON=''

# Newline-separated Email Archive directory names, captured by
# check_archive_root so one ssh round trip serves the menu and the per-account
# check that follows it.
ARCHIVE_ACCOUNTS=''

# Newline-separated folder names of the resolved account, captured by
# list_account_folders so one himalaya IMAP connection serves both the
# --folder menu and the existence check.
FOLDER_NAMES=''

# Newline-separated Synced Folder set of the resolved account, and the
# Account Reconcile drift either side of it, captured by list_synced_folders
# so one auberge call serves the --folder menu, the eligibility check, and
# the drift warning.
SYNCED_FOLDERS=''
DRIFT_ADDED=''
DRIFT_REMOVED=''

# Gate findings, threaded from the gate that computes them to the summary and
# the expunge.
BACKUP_EVIDENCE=''
CUTOFF_DATE=''
ENVELOPE_JSON=''
IMAP_COUNT=0

# Set by count_distinct_message_ids, which reports through globals rather than
# stdout: an abort has to carry which sidecar caused it, and a command
# substitution would run the whole check in a subshell where neither the count nor
# that path survives — leaving the caller to read an empty result as zero
# coverage rather than as a broken sidecar.
ARCHIVE_COUNT=0
UNKEYED_SIDECAR=''

# Set by parse_identity_verdict, same reason as ARCHIVE_COUNT above: the
# verdict is several fields plus a pass/fail, and reporting them on stdout
# would force the caller into a command substitution whose subshell keeps
# only one string.
IDENTITY_STATUS=''
IDENTITY_MISSING=0
IDENTITY_STORE=0
IDENTITY_SYNTHETIC=0
IDENTITY_SHA256=0
IDENTITY_SAMPLE=''

# Set by the fetch_* functions when they cannot answer, so a die-free caller
# (the sweep) can name the reason without re-deriving it. The single-target
# wrappers turn it into the die they have always produced.
FETCH_ERROR=''

# The account-wide sidecar rows gate 3 counts from —
# "path<TAB>folder<TAB>message_id", already window-filtered on the Host — and
# which account they belong to. One Host walk serves every folder of the
# account; see ensure_account_sidecar_rows for why that matters.
SIDECAR_ROWS=''
SIDECAR_ROWS_FOR=''

# Gate 3's verdict for the resolved (account, folder), plus the one count only
# the flagged verdict needs — see probe_folder_coverage for the vocabulary.
PAIR_STATUS=''
ALREADY_DELETED=0

# Set by collect_envelope_ids: the validated ids the expunge will flag, or the
# reason the listing cannot be acted on.
ENVELOPE_IDS=()
IDS_ERROR=''

# Sweep state: one row per classified (account, folder) pair —
# "status<TAB>account<TAB>folder<TAB>count<TAB>detail", status one of
# ready | skip | finding | expunged. The rows drive the summary table, the
# checkpoints, the expunge phase, the final report, and the exit code. Ready
# pairs also park the envelope ids gate 3 counted, keyed "account<TAB>folder",
# so the expunge acts on exactly the listing that passed coverage.
SWEEP_ROWS=()
declare -A SWEEP_IDS=()

# How many pairs gate 3 actually probed; a sweep that probed none has an
# empty target set, which aborts rather than reporting a vacuous success.
SWEEP_PAIRS_PROBED=0

usage() {
  cat <<EOF
Expunge archived mail from an Upstream Mailbox, behind five safety gates.

Usage: ${PROGRAM_NAME} [options]

Options:
  -H, --host HOST            ssh target of the Bichon Host, also passed to
                             \`auberge backup verify --host\` (on a TTY, chosen
                             from \`auberge host list\`)
  -a, --account ADDRESS      himalaya account to expunge from; must be the
                             mailbox email address, since it also names the
                             Email Archive directory (on a TTY, chosen from
                             \`himalaya account list\`). Repeat it with --sweep
                             to sweep only those accounts
  -f, --folder NAME          folder to expunge (on a TTY, chosen from the
                             account's Synced Folders; without one, defaults
                             to ${DEFAULT_FOLDER})
      --sweep                expunge every Synced Folder of every eligible
                             account on the Host in one run — or of the
                             accounts --account names: host gates once,
                             coverage per pair, one summary, two typed
                             checkpoints (Host name, then grand total);
                             excludes --folder
  -w, --window-days DAYS     expunge mail older than DAYS (default: ${DEFAULT_WINDOW_DAYS})
      --archive-path PATH    Email Archive root on the Host
                             (default: ${DEFAULT_ARCHIVE_PATH})
      --no-input             never prompt; requires --host, and --account
                             unless --sweep; refuses the expunge (gates still
                             run)
  -h, --help                 show this help

The expunge requires an interactive TTY. There is deliberately no flag that
skips the typed confirmation. A sweep exits 0 only when no pair was skipped
as a finding; 1 means the sweep report needs reading.
EOF
}

# die <exit-code> <message> [remediation-line...]
die() {
  local code="$1" message="$2"
  shift 2
  printf '%s: error: %s\n' "${PROGRAM_NAME}" "${message}" >&2
  local line
  for line in "$@"; do
    printf '  %s\n' "${line}" >&2
  done
  exit "${code}"
}

# Chrome goes to stderr (clig.dev); stdout carries the outcome line only.
note() {
  printf '%s\n' "$*" >&2
}

step() {
  printf '\n==> %s\n' "$*" >&2
}

interactive() {
  [[ "${no_input}" == false ]] && [[ -t 0 ]]
}

parse_args() {
  while (($# > 0)); do
    case "$1" in
      -H | --host)
        [[ $# -ge 2 ]] || die 2 "$1 needs a value"
        host="$2"
        shift 2
        ;;
      -a | --account)
        [[ $# -ge 2 ]] || die 2 "$1 needs a value"
        requested_accounts+=("$2")
        shift 2
        ;;
      -f | --folder)
        [[ $# -ge 2 ]] || die 2 "$1 needs a value"
        folder="$2"
        shift 2
        ;;
      -w | --window-days)
        [[ $# -ge 2 ]] || die 2 "$1 needs a value"
        window_days="$2"
        shift 2
        ;;
      --archive-path)
        [[ $# -ge 2 ]] || die 2 "$1 needs a value"
        archive_path="$2"
        shift 2
        ;;
      --sweep)
        sweep=true
        shift
        ;;
      --no-input)
        no_input=true
        shift
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        usage >&2
        die 2 "unknown argument: $1"
        ;;
    esac
  done
}

# Present a numbered menu and echo the chosen entry. `select` writes both the
# menu and PS3 to stderr, so command substitution captures the choice alone and
# stdout stays reserved for the outcome line.
choose_from() {
  local label="$1"
  shift
  # `select` over an empty list never enters its body and never reads stdin, so
  # without this the post-loop check would report an empty menu as EOF.
  (($# > 0)) || die 2 "nothing to choose from for ${label}"

  local PS3="${label}: " choice=''
  note ''
  select choice in "$@"; do
    [[ -n "${choice}" ]] && break
    note 'enter the number next to your choice'
  done
  [[ -n "${choice}" ]] || die 2 "stdin closed while choosing ${label}"
  printf '%s' "${choice}"
}

# `auberge host list` is the same registry `auberge backup verify --host`
# resolves against, so a name picked here is guaranteed to mean something to
# gate 1. Whether it is also an ssh target is what check_host_reachable proves.
choose_host() {
  local hosts_json host_names names=()
  hosts_json=$(auberge host list --output json) \
    || die 2 'auberge host list failed' \
      'pass the ssh target directly: --host HOST'

  # jq is read for its exit status here rather than through mapfile: mapfile
  # reports only its own success, so unparseable JSON would reach the operator
  # as "no configured hosts" instead of as a parse failure.
  host_names=$(printf '%s' "${hosts_json}" | jq -r '.[].name') \
    || die 2 'auberge host list returned JSON this script cannot read' \
      'pass the ssh target directly: --host HOST'

  [[ -n "${host_names}" ]] \
    || die 2 'auberge has no configured hosts to choose from' \
      'add one: auberge host add' \
      'or pass the ssh target directly: --host HOST'

  mapfile -t names <<<"${host_names}"
  choose_from 'Bichon Host' "${names[@]}"
}

# The account names in the listing check_tools captured. Callers read jq's exit
# status themselves — same reason as choose_host: a `while read` or a mapfile
# fed by this reports only its own success, so unparseable JSON would reach the
# operator as "no accounts" instead of as a parse failure.
himalaya_account_names() {
  printf '%s' "${HIMALAYA_ACCOUNTS_JSON}" | jq -r '.[].name'
}

# bichon names each Email Archive directory after the mailbox email address,
# with '/' replaced. Keep in step with sanitize_email in
# ansible/roles/bichon/templates/bichon-archive.sh.j2.
archive_dir_for() {
  printf '%s' "${1//\//_}"
}

# Offer only accounts that himalaya knows AND that have an Email Archive
# directory. himalaya account names are arbitrary labels, so nothing forces one
# to equal the mailbox email that names the archive directory — but this script
# passes a single value to both. Intersecting here makes the mismatch
# unselectable instead of surfacing it as a phantom coverage gap in gate 3.
choose_account() {
  local account_names name candidates=()
  account_names=$(himalaya_account_names) \
    || die 2 'himalaya account list returned JSON this script cannot read'

  while IFS= read -r name; do
    [[ -n "${name}" ]] || continue
    grep -qxF -- "$(archive_dir_for "${name}")" <<<"${ARCHIVE_ACCOUNTS}" \
      && candidates+=("${name}")
  done <<<"${account_names}"

  ((${#candidates[@]} > 0)) \
    || die 2 "no himalaya account matches an Email Archive directory on ${host}" \
      "himalaya accounts: $(printf '%s' "${account_names}" | paste -sd' ' -)" \
      "archive directories: $(printf '%s' "${ARCHIVE_ACCOUNTS}" | paste -sd' ' -)" \
      'name the himalaya account after the mailbox email address: bichon keys' \
      'the archive by email, and --account feeds both'

  choose_from 'himalaya account' "${candidates[@]}"
}

# The menu offers only folders that are both on the Upstream Mailbox and a
# Synced Folder — the same intersection principle choose_account applies to
# --account. comm -12 needs both inputs sorted, and both already are:
# reconcile.rs sorts added/removed/unchanged before returning them, and
# FOLDER_NAMES is piped through `sort` in list_account_folders.
choose_folder() {
  local names=()
  mapfile -t names < <(comm -12 <(printf '%s\n' "${SYNCED_FOLDERS}") <(printf '%s\n' "${FOLDER_NAMES}"))

  ((${#names[@]} > 0)) \
    || die 2 "no Synced Folder of ${account} exists on the Upstream Mailbox" \
      "synced folders: $(printf '%s' "${SYNCED_FOLDERS}" | paste -sd' ' -)" \
      "mailbox folders: $(printf '%s' "${FOLDER_NAMES}" | paste -sd' ' -)"

  choose_from 'folder to expunge' "${names[@]}"
}

resolve_host() {
  [[ -n "${host}" ]] && return 0
  interactive || die 2 'missing --host' \
    'a non-interactive run must pass every value as a flag'
  host=$(choose_host)
}

# validate_options has already refused a second --account for a single target,
# so the first entry is the only entry.
resolve_account() {
  if ((${#requested_accounts[@]} > 0)); then
    account="${requested_accounts[0]}"
    return 0
  fi
  interactive || die 2 'missing --account' \
    'a non-interactive run must pass every value as a flag'
  account=$(choose_account)
}

# Unlike --host and --account, an omitted --folder has a documented default,
# so a non-interactive run proceeds with it rather than dying.
resolve_folder() {
  [[ -n "${folder}" ]] && return 0
  if interactive; then
    folder=$(choose_folder)
  else
    folder="${DEFAULT_FOLDER}"
  fi
}

validate_host() {
  # A host starting with '-' would be parsed by ssh as an option, so the
  # character class is a guard, not cosmetics.
  [[ "${host}" =~ ^[A-Za-z0-9][A-Za-z0-9._@-]*$ ]] \
    || die 2 "not a usable ssh target: ${host}" \
      'expected a hostname, ssh_config alias, or user@host'
}

validate_account() {
  [[ "${account}" == *[![:space:]]* ]] \
    || die 2 'account must not be empty'
}

# The options that depend on neither the Host nor the account, so they can
# reject a typo before anything reaches the network.
validate_options() {
  # A sweep's folder set comes from Account Reconcile, never from a flag: a
  # --folder next to --sweep is contradictory intent, and guessing which was
  # meant is how the wrong mailbox gets expunged. --account is different — it
  # narrows the account set the sweep walks without touching how folders are
  # chosen, so the two compose.
  if [[ "${sweep}" == true ]] && [[ -n "${folder}" ]]; then
    die 2 '--sweep takes every Synced Folder of each account; --folder contradicts it' \
      'drop --sweep to target one pair, or drop --folder to sweep every folder' \
      'to sweep some accounts only, repeat --account instead'
  fi

  # One target takes one account. Several of them mean a sweep was intended,
  # and silently expunging the first (or the last) is the wrong-mailbox
  # mistake every gate here exists to prevent.
  if [[ "${sweep}" == false ]] && ((${#requested_accounts[@]} > 1)); then
    die 2 "one target takes one --account, got ${#requested_accounts[@]}" \
      'sweep those accounts instead: --sweep --account A --account B'
  fi

  if ((${#requested_accounts[@]} > 0)); then
    local name
    for name in "${requested_accounts[@]}"; do
      [[ "${name}" == *[![:space:]]* ]] \
        || die 2 'account must not be empty'
    done
  fi

  # Empty means --folder was not passed; resolve_folder supplies the default
  # or the menu answer later, so only a flag-passed value reaches this check.
  # A newline in it would desynchronise the typed-confirmation comparison from
  # what the operator sees on screen.
  [[ "${folder}" != *$'\n'* ]] \
    || die 2 'folder must not contain a newline'

  [[ "${window_days}" =~ ^[1-9][0-9]*$ ]] \
    || die 2 "window-days must be a positive integer, got: ${window_days}"

  [[ "${archive_path}" == /* ]] \
    || die 2 "archive-path must be absolute, got: ${archive_path}"
}

# Where himalaya looks for its config. HIMALAYA_CONFIG wins and holds a
# colon-separated list whose first entry is the primary file — the one whose
# absence triggers the wizard.
himalaya_config_path() {
  if [[ -n "${HIMALAYA_CONFIG:-}" ]]; then
    printf '%s' "${HIMALAYA_CONFIG%%:*}"
    return 0
  fi
  printf '%s' "${XDG_CONFIG_HOME:-${HOME}/.config}/himalaya/config.toml"
}

# Everything that can be checked without knowing the Host or the account. Runs
# before any prompting, so a missing tool costs one line instead of two typed
# answers and two network round trips.
check_tools() {
  step 'Checking tools…'

  # Report every missing tool at once; discovering them one re-run at a time is
  # the failure mode this ordering exists to remove.
  local tool missing=()
  for tool in auberge himalaya jq ssh; do
    command -v "${tool}" >/dev/null 2>&1 || missing+=("${tool}")
  done
  ((${#missing[@]} == 0)) \
    || die 2 "not on PATH: ${missing[*]}" \
      'see the Prerequisites block at the top of this script'

  # Gate 1 needs `auberge backup verify`. clap exits non-zero on an unknown
  # subcommand, which is exactly the signal for "your auberge is too old".
  auberge backup verify --help >/dev/null 2>&1 \
    || die 2 'this auberge has no "backup verify" subcommand' \
      'upgrade auberge: mise up auberge' \
      'gate 1 asserts the archive reached the off-host restic repository;' \
      'there is no weaker substitute for it'

  # Same clap signal, for the subcommand the eligibility check needs.
  auberge bichon reconcile-folders --help >/dev/null 2>&1 \
    || die 2 'this auberge has no "bichon reconcile-folders" subcommand' \
      'upgrade auberge: mise up auberge' \
      'the eligibility check needs it to name the Synced Folder set'

  # Same clap signal, for the identity half of gate 3.
  auberge bichon verify-coverage --help >/dev/null 2>&1 \
    || die 2 'this auberge has no "bichon verify-coverage" subcommand' \
      'upgrade auberge: mise up auberge' \
      'gate 3 proves coverage by message identity against the Bichon store;' \
      'counts alone pass on surplus after a folder'"'"'s first expunge (#400)'

  # With no config himalaya opens its first-run wizard, and that wizard reads
  # /dev/tty rather than stdin — redirecting stdin cannot stop it. So prove the
  # config exists instead of discovering it by blocking on a prompt.
  local himalaya_config
  himalaya_config=$(himalaya_config_path)
  [[ -r "${himalaya_config}" ]] \
    || die 2 'himalaya has no configuration this script can find' \
      "looked for: ${himalaya_config}" \
      'create it: himalaya account configure your.address@example.com' \
      'name the account after the mailbox email address — bichon keys the' \
      'Email Archive by email and --account feeds both' \
      'or point HIMALAYA_CONFIG at an existing config'

  # `account list` reads the config only, opening no IMAP connection, so it is a
  # cheap proof that gate 3 and the expunge will have a usable himalaya.
  # --quiet keeps a successful listing silent; stderr is deliberately left
  # connected so a config error — or any prompt this script failed to
  # anticipate — is visible rather than a silent stall.
  HIMALAYA_ACCOUNTS_JSON=$(himalaya --quiet --output json account list </dev/null) \
    || die 2 "himalaya could not read its accounts from ${himalaya_config}" \
      'himalaya printed the reason above'

  note 'auberge, himalaya, jq, ssh ok'
}

# Every --account has to name a himalaya account, and this is the cheapest place
# to prove it: the listing is already in hand and no ssh has happened yet.
#
# The check matters most to a sweep. Its report has a row per candidate the
# classification saw, and a name matching no candidate produces no row — so
# without this, a typo would leave the operator reading a report that simply
# does not mention the account they asked to expunge, and a run that swept
# nothing would still exit as if it had done the job.
check_requested_accounts() {
  ((${#requested_accounts[@]} > 0)) || return 0

  local account_names name unknown=()
  account_names=$(himalaya_account_names) \
    || die 2 'himalaya account list returned JSON this script cannot read'

  for name in "${requested_accounts[@]}"; do
    grep -qxF -- "${name}" <<<"${account_names}" || unknown+=("${name}")
  done

  ((${#unknown[@]} == 0)) \
    || die 2 "himalaya knows no account named: ${unknown[*]}" \
      "himalaya accounts: $(printf '%s' "${account_names}" | paste -sd' ' -)" \
      '--account must be the mailbox email address, since bichon keys the' \
      'Email Archive by email and one value feeds both'
}

check_host_reachable() {
  step "Checking ssh to ${host}…"

  ssh "${SSH_OPTS[@]}" "${host}" true 2>/dev/null \
    || die 2 "cannot ssh to ${host}" \
      "check: ssh ${host}" \
      'key-based auth is required (this script runs ssh in BatchMode)'

  note "ssh to ${host} ok"
}

# Gate 3 counts archived .eml files by reading the Email Archive over the same
# ssh connection, and it suppresses find's stderr. An unreadable archive root
# therefore produces a bare non-zero exit with no explanation, so prove the
# directory is readable here, where the reason can still be named.
check_archive_root() {
  step "Checking the Email Archive on ${host}…"

  local rc=0
  # shellcheck disable=SC2029  # intentional: %q-quote the path locally, then expand into ssh cmd
  ARCHIVE_ACCOUNTS=$(
    ssh "${SSH_OPTS[@]}" "${host}" \
      "bash -s -- $(printf '%q' "${archive_path}")" <<'REMOTE'
set -euo pipefail
archive_path=$1
[ -d "$archive_path" ] || exit 3
{ [ -r "$archive_path" ] && [ -x "$archive_path" ]; } || exit 4
# Skip dot-directories: bichon keeps its sync cursors in .state alongside the
# per-account directories, and it is not a mailbox.
find "$archive_path" -mindepth 1 -maxdepth 1 -type d ! -name '.*' -printf '%f\n' | sort
REMOTE
  ) || rc=$?

  case "${rc}" in
    0) ;;
    3)
      die 2 "no Email Archive at ${archive_path} on ${host}" \
        "seed it: ssh ${host} sudo systemctl start bichon-archive.service" \
        'or point --archive-path at the right root'
      ;;
    4)
      die 2 "cannot read ${archive_path} on ${host}" \
        'bichon creates the archive 0750 bichon:bichon, so the ssh user has to' \
        'be in that group to count anything in it' \
        "inspect: ssh ${host} stat -c '%U:%G %a' ${archive_path}" \
        "grant: ssh ${host} 'sudo usermod -aG bichon \$(whoami)', then reconnect"
      ;;
    *)
      die 2 "could not inspect ${archive_path} on ${host}"
      ;;
  esac

  [[ -n "${ARCHIVE_ACCOUNTS}" ]] \
    || die 2 "the Email Archive at ${archive_path} on ${host} holds no accounts" \
      "run the archive: ssh ${host} sudo systemctl start bichon-archive.service"

  note "archive accounts: $(printf '%s' "${ARCHIVE_ACCOUNTS}" | paste -sd' ' -)"
}

# An --account passed as a flag skips the intersection the menu applies, so
# check the same coupling here.
check_account_archive() {
  grep -qxF -- "$(archive_dir_for "${account}")" <<<"${ARCHIVE_ACCOUNTS}" \
    || die 2 "no Email Archive directory for ${account} on ${host}" \
      "expected: ${archive_path}/$(archive_dir_for "${account}")" \
      "found: $(printf '%s' "${ARCHIVE_ACCOUNTS}" | paste -sd' ' -)" \
      '--account must be the mailbox email address, since bichon keys the' \
      'archive by email, and it must also name a himalaya account'
}

# Populates FOLDER_NAMES for the --folder menu, check_folder_exists, and the
# sweep — one himalaya IMAP connection serves them all. Returns non-zero with
# FETCH_ERROR set instead of dying: the single target treats any failure as
# fatal (list_account_folders), the sweep as a finding scoped to one account.
# rc 1 means himalaya itself said why on stderr; anything else is self-contained.
fetch_account_folders() {
  FOLDER_NAMES=''
  FETCH_ERROR=''

  # --quiet rather than 2>/dev/null: himalaya prompts on /dev/tty when it
  # needs a secret it cannot read non-interactively, and a discarded stderr
  # turns that prompt into an unexplained stall.
  local folders_json
  if ! folders_json=$(himalaya --quiet --output json folder list --account "${account}"); then
    FETCH_ERROR="himalaya could not list the folders of ${account}"
    return 1
  fi

  # pipefail is set, so a jq failure on unparseable JSON propagates through
  # sort instead of being swallowed.
  if ! FOLDER_NAMES=$(printf '%s' "${folders_json}" | jq -r '.[].name' | sort); then
    FETCH_ERROR='himalaya folder list returned JSON this script cannot read'
    return 2
  fi

  if [[ -z "${FOLDER_NAMES}" ]]; then
    FETCH_ERROR="himalaya reports no folders for ${account}"
    return 2
  fi
}

list_account_folders() {
  local rc=0
  fetch_account_folders || rc=$?
  case "${rc}" in
    0) ;;
    1)
      die 2 "${FETCH_ERROR}" \
        'himalaya printed the reason above'
      ;;
    *)
      die 2 "${FETCH_ERROR}"
      ;;
  esac
}

# The Email Archive can vouch only for folders Bichon continuously ingests, so
# eligibility is the sync_folders set Account Reconcile maintains — asked from
# auberge rather than recomputed here, so the exclusion rules (SPECIAL-USE
# attributes, fallback names, extra_excluded_folders) keep a single owner; a
# dry run PATCHes nothing.
#
# Returns non-zero with FETCH_ERROR set instead of dying — same split as
# fetch_account_folders, and rc 1 again means auberge said why on stderr. An
# empty SYNCED_FOLDERS is rc 0: no eligible folder is a verdict, not a failure,
# and the two callers read it differently (fatal for a --folder run, a benign
# skip for the sweep).
fetch_synced_folders() {
  SYNCED_FOLDERS=''
  DRIFT_ADDED=''
  DRIFT_REMOVED=''
  FETCH_ERROR=''

  local reconcile_json
  if ! reconcile_json=$(auberge bichon reconcile-folders --host "${host}" --account "${account}" --output json); then
    FETCH_ERROR="auberge could not read the Synced Folder set for ${account}"
    return 1
  fi

  local account_count
  if ! account_count=$(printf '%s' "${reconcile_json}" | jq '.accounts | length'); then
    FETCH_ERROR='auberge reconcile-folders returned JSON this script cannot read'
    return 2
  fi
  if ((account_count == 0)); then
    FETCH_ERROR="bichon knows no account ${account}"
    return 2
  fi

  if ! SYNCED_FOLDERS=$(printf '%s' "${reconcile_json}" | jq -r '.accounts[0].unchanged[]?') \
    || ! DRIFT_ADDED=$(printf '%s' "${reconcile_json}" | jq -r '.accounts[0].added[]?') \
    || ! DRIFT_REMOVED=$(printf '%s' "${reconcile_json}" | jq -r '.accounts[0].removed[]?'); then
    FETCH_ERROR='auberge reconcile-folders returned JSON this script cannot read'
    return 2
  fi

  # Drift only blocks once it names the target folder — check_folder_synced
  # re-checks that once --folder is known — so this is a warning, not a gate.
  if [[ -n "${DRIFT_ADDED}" ]] || [[ -n "${DRIFT_REMOVED}" ]]; then
    note "Account Reconcile drift for ${account} since the last --apply:"
    [[ -z "${DRIFT_ADDED}" ]] || note "  not yet synced: $(printf '%s' "${DRIFT_ADDED}" | paste -sd' ' -)"
    [[ -z "${DRIFT_REMOVED}" ]] || note "  pending removal: $(printf '%s' "${DRIFT_REMOVED}" | paste -sd' ' -)"
    note "  fix: auberge bichon reconcile-folders --host ${host} --apply"
  fi
}

list_synced_folders() {
  local rc=0
  fetch_synced_folders || rc=$?
  case "${rc}" in
    0) ;;
    1)
      die 2 "${FETCH_ERROR}" \
        'auberge printed the reason above' \
        'it needs bichon_api_token in its config and the Bichon API reachable'
      ;;
    *)
      die 2 "${FETCH_ERROR}"
      ;;
  esac

  [[ -n "${SYNCED_FOLDERS}" ]] \
    || die 2 "no Synced Folder for ${account} — nothing is eligible for expunge"
}

# IMAP treats every mailbox name but INBOX as case-sensitive, and the server's
# own refusal ("Mailbox doesn't exist: SENT") names no alternative, so the
# near miss is computed client-side where it can be named.
folder_case_insensitive_match() {
  grep -ixF -m1 -- "$1"
}

# A --folder passed as a flag skips what the menu guarantees exists, so check
# it here — the same coupling check_account_archive applies to --account.
check_folder_exists() {
  grep -qxF -- "${folder}" <<<"${FOLDER_NAMES}" && return 0

  local match='' lines=()
  if match=$(folder_case_insensitive_match "${folder}" <<<"${FOLDER_NAMES}"); then
    lines+=("did you mean: ${match} (IMAP folder names are case-sensitive)")
  fi
  lines+=("folders of ${account}: $(printf '%s' "${FOLDER_NAMES}" | paste -sd' ' -)")

  die 2 "no folder named ${folder} for ${account}" "${lines[@]}"
}

# A --folder passed as a flag skips what the menu guarantees is a Synced
# Folder, so re-apply eligibility here — the same coupling check_folder_exists
# applies to existence.
check_folder_synced() {
  if grep -qxF -- "${folder}" <<<"${SYNCED_FOLDERS}"; then
    return 0
  elif grep -qxF -- "${folder}" <<<"${DRIFT_REMOVED}"; then
    die 2 "${folder} is being removed from the Synced Folder set of ${account}" \
      'the operator has excluded it, so its archive coverage is about to go stale' \
      "apply the pending reconcile: auberge bichon reconcile-folders --host ${host} --apply" \
      'then pick a folder the archive still vouches for'
  elif grep -qxF -- "${folder}" <<<"${DRIFT_ADDED}"; then
    die 2 "${folder} is not yet in the Synced Folder set of ${account}" \
      "apply the pending reconcile: auberge bichon reconcile-folders --host ${host} --apply" \
      "then backfill it: auberge bichon rescan --host ${host} --account ${account}" \
      're-run once the archive has ingested it'
  else
    die 2 "${folder} is not a Synced Folder of ${account}" \
      'bichon deliberately does not archive it, so the Email Archive cannot' \
      'vouch for its contents and an expunge would delete mail that may exist' \
      'nowhere else' \
      "synced folders: $(printf '%s' "${SYNCED_FOLDERS}" | paste -sd' ' -)"
  fi
}

# Guard a value that crossed back from the Host before it reaches (( )).
# Arithmetic evaluation expands command substitutions, so a non-numeric value
# there would be more than a wrong answer.
require_number() {
  local label="$1" value="$2"
  [[ "${value}" =~ ^[0-9]+$ ]] \
    || die 1 "${host} returned a non-numeric ${label}: ${value}"
}

# Reads "<sidecar path>\t<message id>" rows on stdin and sets ARCHIVE_COUNT to
# the number of distinct ids. Distinct, not row count: Bichon regenerates
# envelope identifiers on re-import, so one message can hold several
# `<envelope-id>.eml` copies, and counting files credits the archive with
# coverage it does not have.
#
# A row with no id means a sidecar predating Message-ID keying. That is an
# unknown, not a zero: it returns non-zero with the path in UNKEYED_SIDECAR
# rather than falling back to counting files, which is the inflation this gate
# exists to refuse.
count_distinct_message_ids() {
  local path message_id
  local -A distinct=()

  ARCHIVE_COUNT=0
  UNKEYED_SIDECAR=''

  while IFS=$'\t' read -r path message_id; do
    [[ -n "${path}" ]] || continue
    if [[ -z "${message_id}" ]]; then
      UNKEYED_SIDECAR="${path}"
      return 1
    fi
    distinct["${message_id}"]=1
  done

  ARCHIVE_COUNT="${#distinct[@]}"
}

# Reads an `auberge bichon verify-coverage --output json` verdict on stdin and
# fills the IDENTITY_* globals — same globals-not-stdout shape as
# count_distinct_message_ids, and the same reason: several fields and a
# pass/fail have to survive out of one call.
#
# auberge's JSON is the only interface this script has to the identity half
# of gate 3, so a verdict this function cannot read must fail the gate
# loudly, not be read as covered. jq failing on non-JSON input, a missing or
# unrecognised status, and a count field that is not a bare non-negative
# integer are all the same failure: a half-parsed verdict must not pass a
# gate.
parse_identity_verdict() {
  IDENTITY_STATUS=''
  IDENTITY_MISSING=0
  IDENTITY_STORE=0
  IDENTITY_SYNTHETIC=0
  IDENTITY_SHA256=0
  IDENTITY_SAMPLE=''

  local tsv
  if ! tsv=$(jq -r '[
      .status,
      (.missing | length),
      .store_messages,
      .unverifiable.store_synthetic,
      .unverifiable.archive_sha256,
      ([.missing[0:3][].message_id] | join(" "))
    ] | @tsv'); then
    return 1
  fi

  local status missing store synthetic sha256 sample
  IFS=$'\t' read -r status missing store synthetic sha256 sample <<<"${tsv}"

  [[ "${status}" == 'covered' ]] || [[ "${status}" == 'gap' ]] || return 1
  [[ "${missing}" =~ ^[0-9]+$ ]] || return 1
  [[ "${store}" =~ ^[0-9]+$ ]] || return 1
  [[ "${synthetic}" =~ ^[0-9]+$ ]] || return 1
  [[ "${sha256}" =~ ^[0-9]+$ ]] || return 1

  IDENTITY_STATUS="${status}"
  IDENTITY_MISSING="${missing}"
  IDENTITY_STORE="${store}"
  IDENTITY_SYNTHETIC="${synthetic}"
  IDENTITY_SHA256="${sha256}"
  IDENTITY_SAMPLE="${sample}"
}

# Fetches the account's window-filtered sidecar rows
# ("path<TAB>folder<TAB>message_id") once and caches them, so every folder of
# the account shares one Host walk. The walk used to run per (account, folder)
# pair with one jq and one awk spawned per sidecar file — an
# O(folders × sidecars) process storm the sweep multiplied until it dominated
# the runtime (measured 55× slower than one batched pass on the real archive).
# The remote side batches jq over every sidecar via xargs and filters the
# YYYY/MM window in a single awk pass.
#
# Archive paths encode the message Date as YYYY/MM; folder identity and
# message identity both live in the sidecar, which is why the rows carry the
# folder for sidecar_rows_for_folder to scope by. An unreadable or
# unparseable sidecar fails the whole fetch loudly (jq names it on stderr,
# pipefail carries it out), exactly as the per-file check did — a broken
# count must never pass as a coverage verdict. Returns non-zero with
# FETCH_ERROR set.
ensure_account_sidecar_rows() {
  if [[ "${SIDECAR_ROWS_FOR}" == "${account}" ]]; then
    return 0
  fi

  local cutoff_ym archive_dir
  cutoff_ym=$(date_offset "${window_days}" '+%Y/%m')
  archive_dir="${archive_path}/$(archive_dir_for "${account}")"

  FETCH_ERROR=''
  # Remote args go through printf %q so ${archive_dir} cannot break out of
  # argument quoting and execute on the Host. The heredoc body is
  # single-quoted ('REMOTE') to disable local expansion — only positional
  # parameters are used.
  # shellcheck disable=SC2029  # intentional: %q-quote args locally, then expand into ssh cmd
  if ! SIDECAR_ROWS=$(
    ssh "${SSH_OPTS[@]}" "${host}" \
      "bash -s -- $(printf '%q ' "${archive_dir}" "${cutoff_ym}")" <<'REMOTE'
set -euo pipefail
archive_dir=$1; cutoff_ym=$2
find "$archive_dir" -regextype posix-extended \
  -regex '.*/[0-9]{4}/[0-9]{2}/[^/]+\.meta\.json' -print0 \
  | xargs -0 -r jq -r '[input_filename, .folder, (.message_id // "")] | @tsv' \
  | awk -F'\t' -v cutoff="$cutoff_ym" '{
      n = split($1, p, "/")
      ym = p[n - 2] "/" p[n - 1]
      if (!(ym > cutoff)) print
    }'
REMOTE
  ); then
    SIDECAR_ROWS=''
    SIDECAR_ROWS_FOR=''
    FETCH_ERROR="could not read the archived sidecars of ${account} on ${host}"
    return 1
  fi

  SIDECAR_ROWS_FOR="${account}"
}

# Filters the account-wide sidecar rows on stdin down to one folder's
# "path<TAB>message_id" rows — the shape count_distinct_message_ids consumes.
# The IMAP query is folder-scoped, so the archive count must be too: counting
# account-wide sidecars would over-count and pass the gate while the target
# folder is only partially archived. Exact match; the filter rides ENVIRON
# because awk -v mangles backslashes.
sidecar_rows_for_folder() {
  FOLDER_FILTER="$1" awk -F'\t' '$2 == ENVIRON["FOLDER_FILTER"] { print $1 "\t" $3 }'
}

# Print UTC date offset by -$1 days, formatted with $2.
# GNU coreutils (Linux) and BSD (macOS) take incompatible flags; dispatch on
# whichever the operator's `date` binary accepts.
date_offset() {
  if date -u -d "-1 day" '+%Y' >/dev/null 2>&1; then
    date -u -d "-$1 days" "$2"
  else
    date -u -v-"$1"d "$2"
  fi
}

# Gate 1 — the archive is in the off-host restic repository.
gate_backup_verified() {
  if [[ "${sweep}" == true ]]; then
    step 'Sweep gate 1/2 — off-host backup…'
  else
    step 'Gate 1/5 — off-host backup…'
  fi

  local verify_output
  if ! verify_output=$(auberge backup verify --host "${host}" --app bichon 2>&1); then
    printf '%s\n' "${verify_output}" >&2
    die 1 'the bichon archive is not in a fresh off-host snapshot' \
      'run: auberge backup sync --host '"${host}" \
      'expunging now would delete mail that exists in only one place'
  fi

  printf '%s\n' "${verify_output}" >&2
  BACKUP_EVIDENCE="${verify_output}"
}

# Gate 2 — the hourly archive run succeeded, recently.
gate_archive_fresh() {
  if [[ "${sweep}" == true ]]; then
    step 'Sweep gate 2/2 — archive freshness…'
  else
    step 'Gate 2/5 — archive freshness…'
  fi

  # Age is computed on the Host: its `date` is GNU, and systemd's timestamp
  # format is only reliably parseable there.
  local status
  status=$(
    ssh "${SSH_OPTS[@]}" "${host}" 'bash -s' <<'REMOTE'
set -euo pipefail
result=$(systemctl show bichon-archive.service --property=Result --value)
exit_ts=$(systemctl show bichon-archive.service --property=ExecMainExitTimestamp --value)
printf 'result=%s\n' "$result"
if [ -n "$exit_ts" ]; then
  printf 'age_seconds=%s\n' "$(($(date +%s) - $(date -d "$exit_ts" +%s)))"
fi
REMOTE
  )

  local result age_seconds
  result=$(printf '%s\n' "${status}" | sed -n 's/^result=//p')
  age_seconds=$(printf '%s\n' "${status}" | sed -n 's/^age_seconds=//p')

  # shellcheck disable=SC2029  # intentional: ARCHIVE_MAX_AGE_HOURS is a local constant
  ssh "${SSH_OPTS[@]}" "${host}" \
    "journalctl -u bichon-archive.service --since=-${ARCHIVE_MAX_AGE_HOURS}h --no-pager | tail -5" >&2 \
    || true

  [[ -n "${age_seconds}" ]] \
    || die 1 'bichon-archive.service has never completed a run' \
      "start it: ssh ${host} sudo systemctl start bichon-archive.service"

  require_number 'archive run age' "${age_seconds}"

  [[ "${result}" == 'success' ]] \
    || die 1 "the last bichon-archive.service run ended with Result=${result}" \
      "read the failure: ssh ${host} journalctl -u bichon-archive.service -n 50" \
      'a failed run means the archive is incomplete for an unknown set of mail'

  local max_age_seconds=$((ARCHIVE_MAX_AGE_HOURS * 3600))
  ((age_seconds <= max_age_seconds)) \
    || die 1 "the last archive run finished $((age_seconds / 3600))h ago (limit ${ARCHIVE_MAX_AGE_HOURS}h)" \
      "check the timer: ssh ${host} systemctl list-timers bichon-archive.timer"

  note "last run succeeded $((age_seconds / 60))m ago"
}

# Gate 3's checks for the resolved (account, folder): every probe, no verdict
# rendering. Sets PAIR_STATUS and the evidence the verdicts and the expunge
# need (ENVELOPE_JSON, IMAP_COUNT, ARCHIVE_COUNT, CUTOFF_DATE):
#
#   ok            every in-window message has an archived counterpart
#   empty         the window holds no IMAP messages
#   flagged       ALREADY_DELETED message(s) already carry \Deleted
#   unkeyed       UNKEYED_SIDECAR predates Message-ID keying
#   gap           ARCHIVE_COUNT < IMAP_COUNT
#   identity-gap  the store holds in-window message(s) the archive cannot
#                 vouch for by identity
#   err-list      himalaya could not list the window
#   err-flagged   himalaya could not list \Deleted-flagged mail
#   err-sidecars  ssh or the Host could not read the archived sidecars
#   err-identity  auberge could not produce an identity verdict
#
# Always returns 0 — the status is the result. The single target dies on
# anything but ok (render_pair_verdict_or_die); the sweep records a row.
probe_folder_coverage() {
  PAIR_STATUS=''
  ALREADY_DELETED=0
  ENVELOPE_JSON=''
  IMAP_COUNT=0
  ARCHIVE_COUNT=0

  CUTOFF_DATE=$(date_offset "${window_days}" '+%Y-%m-%d')

  # --output json: the default table format adds headers and box drawing, so
  # `wc -l` is off by a few. jq counts envelopes precisely. The filter query is
  # a trailing positional in himalaya, so it must come after every flag.
  # --quiet rather than 2>/dev/null: himalaya prompts on /dev/tty when it needs
  # a secret it cannot read non-interactively, and a discarded stderr turns that
  # prompt into an unexplained stall.
  if ! ENVELOPE_JSON=$(himalaya --quiet --output json envelope list \
    --account "${account}" \
    --folder "${folder}" \
    --page-size "${PAGE_SIZE}" \
    before "${CUTOFF_DATE}"); then
    PAIR_STATUS='err-list'
    return 0
  fi

  IMAP_COUNT=$(printf '%s' "${ENVELOPE_JSON}" | jq 'length')
  note "IMAP messages in window: ${IMAP_COUNT}"

  if ((IMAP_COUNT == 0)); then
    PAIR_STATUS='empty'
    return 0
  fi

  # `folder expunge` removes every \Deleted-flagged message in the folder, not
  # only the ones this script flags. A pre-existing flag would therefore be
  # collateral damage, so refuse rather than widen the blast radius.
  local deleted_json
  if ! deleted_json=$(himalaya --quiet --output json envelope list \
    --account "${account}" \
    --folder "${folder}" \
    --page-size "${PAGE_SIZE}" \
    flag deleted); then
    PAIR_STATUS='err-flagged'
    return 0
  fi
  ALREADY_DELETED=$(printf '%s' "${deleted_json}" | jq 'length')

  if ((ALREADY_DELETED > 0)); then
    PAIR_STATUS='flagged'
    return 0
  fi

  # One account-wide Host walk serves every folder of the account; the rows
  # come back window-filtered and folder-tagged, and the folder scoping the
  # IMAP query already applied happens locally.
  if ! ensure_account_sidecar_rows; then
    PAIR_STATUS='err-sidecars'
    return 0
  fi

  local sidecar_rows
  sidecar_rows=$(printf '%s' "${SIDECAR_ROWS}" | sidecar_rows_for_folder "${folder}")

  if ! count_distinct_message_ids <<<"${sidecar_rows}"; then
    PAIR_STATUS='unkeyed'
    return 0
  fi

  note "archived messages in window: ${ARCHIVE_COUNT}"

  if ((ARCHIVE_COUNT < IMAP_COUNT)); then
    PAIR_STATUS='gap'
    return 0
  fi

  # The count above only bounds the live mailbox: it cannot see below the
  # Archive Cursor's date watermark, where a moved or backdated message can
  # arrive unarchived and hide behind the surplus an append-only archive
  # accumulates after a folder's first expunge (#400). Identity proves the
  # archive holds what the UID-synced Bichon store holds, which the
  # watermark cannot fool.
  local verdict_json rc=0
  verdict_json=$(auberge bichon verify-coverage \
    --host "${host}" \
    --account "${account}" \
    --folder "${folder}" \
    --before "${CUTOFF_DATE}" \
    --archive-path "${archive_path}" \
    --output json) || rc=$?
  if ((rc > 1)); then
    PAIR_STATUS='err-identity'
    return 0
  fi
  if ! parse_identity_verdict <<<"${verdict_json}"; then
    PAIR_STATUS='err-identity'
    return 0
  fi
  if [[ "${IDENTITY_STATUS}" != 'covered' ]]; then
    PAIR_STATUS='identity-gap'
    return 0
  fi
  note "identity ok (${IDENTITY_STORE} store message(s) archived)"

  note "coverage ok (${ARCHIVE_COUNT} >= ${IMAP_COUNT})"
  PAIR_STATUS='ok'
}

# Renders a non-ok PAIR_STATUS as the die the single target has always
# produced; the sweep never calls this — it records a row instead.
render_pair_verdict_or_die() {
  case "${PAIR_STATUS}" in
    ok) ;;
    empty)
      die 1 "no IMAP messages in ${folder} older than ${window_days} days" \
        'nothing to expunge — check --folder and --account'
      ;;
    flagged)
      die 1 "${ALREADY_DELETED} message(s) in ${folder} already carry the deleted flag" \
        "inspect them: himalaya envelope list --account ${account} --folder ${folder} flag deleted" \
        'clear or expunge them separately — this script only expunges what it flags'
      ;;
    unkeyed)
      die 1 "an archived sidecar carries no message_id: ${UNKEYED_SIDECAR}" \
        'that sidecar predates Message-ID keying, so this gate cannot tell whether' \
        'it covers a message it has already counted' \
        "backfill it: ssh ${host} sudo systemctl start bichon-archive.service" \
        'one run repairs every sidecar in the account; re-run this script after it'
      ;;
    gap)
      die 1 "coverage gap: archive has ${ARCHIVE_COUNT} messages, IMAP has ${IMAP_COUNT} messages" \
        "read the journal: ssh ${host} journalctl -u bichon-archive.service" \
        "check ${folder} is a Synced Folder: auberge bichon reconcile-folders --host ${host}" \
        "check the sidecars are group-readable: ssh ${host} find ${archive_path} -type f ! -perm -g=r" \
        'do not expunge until the counts reconcile'
      ;;
    identity-gap)
      if ((IDENTITY_MISSING > 0)); then
        die 1 "identity gap: ${IDENTITY_MISSING} of ${IDENTITY_STORE} store message(s) in ${folder} have no archived counterpart" \
          "e.g. ${IDENTITY_SAMPLE}" \
          'these sit below the Archive Cursor, where the hourly run never looks' \
          'again — the count surplus after a prior expunge masked them' \
          "backfill them: auberge bichon rescan --host ${host} --account ${account}" \
          're-run this script once the rescan has ingested them'
      else
        die 1 "${IDENTITY_SYNTHETIC} header-less store message(s) in ${folder} out-count ${IDENTITY_SHA256} sha256-keyed sidecar(s)" \
          'identity cannot match header-less messages to a sidecar one-to-one' \
          "backfill them: auberge bichon rescan --host ${host} --account ${account}" \
          're-run this script once the rescan has ingested them'
      fi
      ;;
    err-identity)
      die 1 "auberge could not verify ${folder} coverage by identity" \
        'auberge printed the reason above' \
        'it needs bichon_api_token in its config, the Bichon API reachable, and ssh to the Host'
      ;;
    err-list)
      die 1 "himalaya could not list ${folder} for ${account}" \
        'check the account exists: himalaya account list' \
        "check the folder exists: himalaya folder list --account ${account}"
      ;;
    err-flagged)
      die 1 "himalaya could not list deleted-flagged mail in ${folder}" \
        'this script must know what the expunge would take along, so it will' \
        'not continue without that answer'
      ;;
    err-sidecars)
      die 1 "could not read the archived sidecars for ${folder} on ${host}" \
        'ssh or the Host printed the reason above' \
        "if a sidecar was unreadable: ssh ${host} find ${archive_path} -type f ! -perm -g=r" \
        "if ssh itself failed: ssh ${host} true"
      ;;
    *)
      die 1 "gate 3 produced a status this script does not know: ${PAIR_STATUS}"
      ;;
  esac
}

# Gate 3 — every in-window IMAP message has an archived counterpart, proven
# two ways: by count against the live mailbox, and by Message-ID against the
# UID-synced Bichon store.
# Sets IMAP_COUNT, ENVELOPE_JSON and ARCHIVE_COUNT; the expunge reuses the same
# envelope set, so the messages counted here are exactly the messages deleted
# later.
gate_folder_coverage() {
  step "Gate 3/5 — coverage for ${folder} older than ${window_days} days…"
  probe_folder_coverage
  render_pair_verdict_or_die
}

# Gate 4 — show the operator exactly what is about to happen.
print_summary() {
  step 'Gate 4/5 — summary'

  cat >&2 <<EOF
  account         ${account}
  folder          ${folder}
  window          older than ${window_days} days (before ${CUTOFF_DATE})
  messages        ${IMAP_COUNT}
  Host            ${host}
  archive root    ${archive_path}

off-host backup evidence:
$(printf '%s\n' "${BACKUP_EVIDENCE}" | sed 's/^/  /')

commands to be run against ${account}:
  himalaya flag add --account ${account} --folder ${folder} <${IMAP_COUNT} uids> deleted
  himalaya folder expunge --account ${account} ${folder}

The uids are the ${IMAP_COUNT} envelopes listed in gate 3 — no second listing
happens, so nothing that arrived since then can be caught up in the expunge.
EOF
}

# Gate 5 — typed confirmation. TTY-only by construction.
confirm_or_abort() {
  step 'Gate 5/5 — confirmation'

  printf 'This permanently deletes %s message(s) from %s.\n' "${IMAP_COUNT}" "${account}" >&2
  printf 'Type the folder name (%s) to proceed, anything else to abort: ' "${folder}" >&2

  local typed
  IFS= read -r typed || die 1 'stdin closed before confirmation — nothing expunged'

  [[ "${typed}" == "${folder}" ]] \
    || die 1 'confirmation did not match the folder name — nothing expunged'
}

# Fills ENVELOPE_IDS from ENVELOPE_JSON after proving the listing still holds
# exactly the IMAP_COUNT messages gate 3 counted and that every id is numeric —
# himalaya reads any non-numeric positional as a flag name, so a stray value
# would silently become a flag instead of a uid. Returns non-zero with
# IDS_ERROR set; the single target dies on it, the sweep records a finding.
collect_envelope_ids() {
  ENVELOPE_IDS=()
  IDS_ERROR=''
  mapfile -t ENVELOPE_IDS < <(printf '%s' "${ENVELOPE_JSON}" | jq -r '.[].id')

  if ((${#ENVELOPE_IDS[@]} != IMAP_COUNT)); then
    IDS_ERROR="envelope ids (${#ENVELOPE_IDS[@]}) do not match the counted messages (${IMAP_COUNT})"
    return 1
  fi

  local id
  for id in "${ENVELOPE_IDS[@]}"; do
    if ! [[ "${id}" =~ ^[0-9]+$ ]]; then
      IDS_ERROR="himalaya returned a non-numeric envelope id: ${id}"
      return 1
    fi
  done
}

# Flags the given ids \Deleted and expunges the folder: $1 account, $2 folder,
# the rest ids. Returns 1 when flagging failed (nothing was deleted) and 2
# when the expunge failed after the flags landed — the folder then holds
# \Deleted mail, which is exactly what the pre-existing-flag check refuses on
# the next run, so a partial failure cannot compound silently.
expunge_ids() {
  local target_account="$1" target_folder="$2"
  shift 2

  step "Flagging $# message(s) as deleted…"
  himalaya flag add \
    --account "${target_account}" \
    --folder "${target_folder}" \
    "$@" deleted >&2 || return 1

  step "Expunging ${target_folder}…"
  himalaya folder expunge --account "${target_account}" "${target_folder}" >&2 || return 2
}

expunge() {
  collect_envelope_ids \
    || die 1 "${IDS_ERROR}" \
      'refusing to act on a listing that changed underfoot'

  local rc=0
  expunge_ids "${account}" "${folder}" "${ENVELOPE_IDS[@]}" || rc=$?
  case "${rc}" in
    0) ;;
    1)
      die 1 "himalaya could not flag the messages in ${folder} — nothing was deleted" \
        'himalaya printed the reason above'
      ;;
    *)
      die 1 "the flags landed but the expunge of ${folder} failed" \
        "inspect what is still flagged: himalaya envelope list --account ${account} --folder ${folder} flag deleted" \
        'a re-run will refuse the folder until those flags are cleared or expunged'
      ;;
  esac
}

# Classifies every account either side of the sweep could see. $1 is the
# newline-separated himalaya account names, $2 the Email Archive directory
# names, $3 the accounts --account named — empty for a Host-wide sweep. Emits
# one "class<TAB>name" row per in-scope candidate:
#
#   eligible    a himalaya account whose Email Archive directory exists —
#               the same intersection choose_account offers as a menu
#   no-archive  a himalaya account the archive cannot vouch for
#   no-account  an archive directory no himalaya account can reach — a
#               decommissioned mailbox, not an error
#
# A scoped sweep emits no no-account row at all: scope is a set of himalaya
# account names, and that class is defined by having none to match. The
# membership test runs against the account name, not its archive directory, so
# --account keeps meaning the same thing it means everywhere else.
sweep_classify_accounts() {
  local himalaya_names="$1" archive_dirs="$2" requested="${3:-}"

  local name
  while IFS= read -r name; do
    [[ -n "${name}" ]] || continue
    if [[ -n "${requested}" ]] && ! grep -qxF -- "${name}" <<<"${requested}"; then
      continue
    fi
    if grep -qxF -- "$(archive_dir_for "${name}")" <<<"${archive_dirs}"; then
      printf 'eligible\t%s\n' "${name}"
    else
      printf 'no-archive\t%s\n' "${name}"
    fi
  done <<<"${himalaya_names}"

  [[ -z "${requested}" ]] || return 0

  local dir matched
  while IFS= read -r dir; do
    [[ -n "${dir}" ]] || continue
    matched=false
    while IFS= read -r name; do
      [[ -n "${name}" ]] || continue
      if [[ "$(archive_dir_for "${name}")" == "${dir}" ]]; then
        matched=true
        break
      fi
    done <<<"${himalaya_names}"
    if [[ "${matched}" == false ]]; then
      printf 'no-account\t%s\n' "${dir}"
    fi
  done <<<"${archive_dirs}"
}

# Classifies every folder the sweep could see for one account. $1 is the
# Synced Folder set, $2 the Upstream Mailbox folder names (both sorted, as
# their producers guarantee — comm needs that), $3/$4 the reconcile drift.
# Emits "class<TAB>folder" rows:
#
#   eligible          synced AND on the Upstream Mailbox — the same
#                     intersection choose_folder offers as a menu
#   missing-upstream  a Synced Folder the Upstream Mailbox no longer has
#   drift-added       on the Upstream Mailbox, not yet synced
#   drift-removed     pending removal from the Synced Folder set
sweep_classify_folders() {
  local synced="$1" upstream="$2" drift_added="$3" drift_removed="$4"

  local name
  while IFS= read -r name; do
    [[ -n "${name}" ]] || continue
    printf 'eligible\t%s\n' "${name}"
  done < <(comm -12 <(printf '%s\n' "${synced}") <(printf '%s\n' "${upstream}"))

  while IFS= read -r name; do
    [[ -n "${name}" ]] || continue
    printf 'missing-upstream\t%s\n' "${name}"
  done < <(comm -23 <(printf '%s\n' "${synced}") <(printf '%s\n' "${upstream}"))

  while IFS= read -r name; do
    [[ -n "${name}" ]] || continue
    printf 'drift-added\t%s\n' "${name}"
  done <<<"${drift_added}"

  while IFS= read -r name; do
    [[ -n "${name}" ]] || continue
    printf 'drift-removed\t%s\n' "${name}"
  done <<<"${drift_removed}"
}

# Appends one sweep row: $1 status, $2 account, $3 folder, $4 count, $5 detail.
sweep_record() {
  SWEEP_ROWS+=("$(printf '%s\t%s\t%s\t%s\t%s' "$1" "$2" "$3" "$4" "$5")")
}

# Prints the SWEEP_ROWS holding the given status, one per line.
sweep_rows_with_status() {
  local wanted="$1" row
  for row in "${SWEEP_ROWS[@]}"; do
    if [[ "${row}" == "${wanted}"$'\t'* ]]; then
      printf '%s\n' "${row}"
    fi
  done
}

# Sums the count field of the sweep rows on stdin.
sweep_sum_counts() {
  local _status _account _folder count _detail total=0
  while IFS=$'\t' read -r _status _account _folder count _detail; do
    if [[ "${count}" =~ ^[0-9]+$ ]]; then
      total=$((total + count))
    fi
  done
  printf '%s' "${total}"
}

# Renders the sweep rows on stdin as the report body, one aligned line each.
# Findings shout so a skimmed report cannot mistake one for a benign skip.
render_sweep_rows() {
  local status account_ folder_ count detail label
  while IFS=$'\t' read -r status account_ folder_ count detail; do
    [[ -n "${status}" ]] || continue
    label="${status}"
    if [[ "${status}" == 'finding' ]]; then
      label='FINDING'
    fi
    printf '  %-9s %-26s %-16s %8s  %s\n' \
      "${label}" "${account_}" "${folder_}" "${count}" "${detail}"
  done
}

# Renders the whole row set under a step title.
sweep_render_report() {
  step "$1"
  {
    printf '  %-9s %-26s %-16s %8s  %s\n' 'STATUS' 'ACCOUNT' 'FOLDER' 'MESSAGES' 'DETAIL'
    printf '%s\n' "${SWEEP_ROWS[@]}" | render_sweep_rows
  } >&2
}

# Probes gate 3 for the resolved (account, folder) and records the row. A
# ready pair also parks its envelope ids, so the expunge phase acts on exactly
# the listing that passed coverage — same no-second-listing invariant as the
# single target.
sweep_probe_pair() {
  note ''
  note "[${account}] ${folder}: coverage older than ${window_days} days…"

  SWEEP_PAIRS_PROBED=$((SWEEP_PAIRS_PROBED + 1))
  probe_folder_coverage

  case "${PAIR_STATUS}" in
    ok) ;;
    empty)
      sweep_record skip "${account}" "${folder}" 0 'nothing in window'
      return 0
      ;;
    flagged)
      sweep_record finding "${account}" "${folder}" 0 \
        "${ALREADY_DELETED} message(s) already flagged deleted — clear or expunge them separately"
      return 0
      ;;
    unkeyed)
      sweep_record finding "${account}" "${folder}" 0 \
        "unkeyed sidecar ${UNKEYED_SIDECAR} — backfill: sudo systemctl start bichon-archive.service"
      return 0
      ;;
    gap)
      sweep_record finding "${account}" "${folder}" 0 \
        "coverage gap: archive ${ARCHIVE_COUNT} < IMAP ${IMAP_COUNT} — do not expunge until they reconcile"
      return 0
      ;;
    identity-gap)
      if ((IDENTITY_MISSING > 0)); then
        sweep_record finding "${account}" "${folder}" 0 \
          "identity gap: ${IDENTITY_MISSING} store message(s) unarchived — rescan the account, then re-run"
      else
        sweep_record finding "${account}" "${folder}" 0 \
          'identity gap: header-less messages out-count sha256 sidecars — rescan the account'
      fi
      return 0
      ;;
    err-identity)
      sweep_record finding "${account}" "${folder}" 0 'auberge could not produce an identity verdict'
      return 0
      ;;
    err-list)
      sweep_record finding "${account}" "${folder}" 0 'himalaya could not list the folder'
      return 0
      ;;
    err-flagged)
      sweep_record finding "${account}" "${folder}" 0 'himalaya could not list deleted-flagged mail'
      return 0
      ;;
    err-sidecars)
      sweep_record finding "${account}" "${folder}" 0 'could not read the archived sidecars on the Host'
      return 0
      ;;
    *)
      die 1 "gate 3 produced a status this script does not know: ${PAIR_STATUS}"
      ;;
  esac

  if ! collect_envelope_ids; then
    sweep_record finding "${account}" "${folder}" 0 "${IDS_ERROR}"
    return 0
  fi

  SWEEP_IDS["${account}"$'\t'"${folder}"]="${ENVELOPE_IDS[*]}"
  sweep_record ready "${account}" "${folder}" "${IMAP_COUNT}" ''
}

# Probes every folder of the resolved account. Account-level fetch failures
# become findings scoped to the account; an empty Synced Folder set is a
# benign skip, exactly as an empty window is for a pair.
sweep_probe_account() {
  step "Probing ${account}…"

  if ! fetch_account_folders; then
    sweep_record finding "${account}" '-' 0 "${FETCH_ERROR}"
    return 0
  fi

  if ! fetch_synced_folders; then
    sweep_record finding "${account}" '-' 0 "${FETCH_ERROR}"
    return 0
  fi

  if [[ -z "${SYNCED_FOLDERS}" ]]; then
    note "no Synced Folder for ${account} — nothing eligible"
    sweep_record skip "${account}" '-' 0 'no Synced Folders'
    return 0
  fi

  # Fetch the account's sidecar rows here rather than inside the first pair:
  # a Host that cannot be read is one account-level finding, not the same
  # failed ssh retried once per folder.
  if ! ensure_account_sidecar_rows; then
    sweep_record finding "${account}" '-' 0 "${FETCH_ERROR}"
    return 0
  fi

  local class name
  while IFS=$'\t' read -r class name; do
    [[ -n "${class}" ]] || continue
    case "${class}" in
      eligible)
        folder="${name}"
        sweep_probe_pair
        ;;
      missing-upstream)
        sweep_record skip "${account}" "${name}" 0 'a Synced Folder the Upstream Mailbox no longer has'
        ;;
      drift-added)
        sweep_record skip "${account}" "${name}" 0 'not yet synced — apply the pending reconcile first'
        ;;
      drift-removed)
        sweep_record skip "${account}" "${name}" 0 'pending removal from the Synced Folder set'
        ;;
    esac
  done < <(sweep_classify_folders "${SYNCED_FOLDERS}" "${FOLDER_NAMES}" "${DRIFT_ADDED}" "${DRIFT_REMOVED}")
}

# What gate 4 calls the sweep's target set, so the operator sees whether the
# run was narrowed before vouching for it.
sweep_scope_label() {
  if ((${#requested_accounts[@]} == 0)); then
    printf 'every eligible account on %s' "${host}"
  else
    printf '%s account(s) named by --account: %s' \
      "${#requested_accounts[@]}" "${requested_accounts[*]}"
  fi
}

# The sweep's gate 4: the whole row set, then the run parameters and the
# backup evidence gate 1 produced — everything the two checkpoints ask the
# operator to vouch for.
sweep_print_summary() {
  sweep_render_report 'Sweep summary'
  cat >&2 <<EOF

  Host            ${host}
  scope           $(sweep_scope_label)
  window          older than ${window_days} days (before ${CUTOFF_DATE})
  archive root    ${archive_path}

off-host backup evidence:
$(printf '%s\n' "${BACKUP_EVIDENCE}" | sed 's/^/  /')
EOF
}

# The sweep's gate 5: two typed checkpoints instead of one typed folder name
# per pair (ADR-0007, amendment 2026-08-03). Scope binds the sweep to a
# machine; magnitude is a number the operator must copy from the summary,
# proving it was read. TTY-only by construction, like confirm_or_abort.
sweep_confirm_or_abort() {
  local ready accounts account_count folder_count grand_total
  ready=$(sweep_rows_with_status ready)
  accounts=$(printf '%s\n' "${ready}" | cut -f2 | sort -u)
  account_count=$(printf '%s\n' "${accounts}" | grep -c . || true)
  folder_count=$(printf '%s\n' "${ready}" | grep -c . || true)
  grand_total=$(printf '%s\n' "${ready}" | sweep_sum_counts)

  local typed

  step 'Checkpoint 1/2 — scope'
  printf 'This sweep deletes mail from %s account(s) on %s:\n' "${account_count}" "${host}" >&2
  printf '%s\n' "${accounts}" | sed 's/^/  /' >&2
  printf 'Type the Bichon Host name (%s) to proceed, anything else to abort: ' "${host}" >&2
  IFS= read -r typed || die 1 'stdin closed before the scope checkpoint — nothing expunged'
  [[ "${typed}" == "${host}" ]] \
    || die 1 'checkpoint did not match the Host name — nothing expunged'

  step 'Checkpoint 2/2 — magnitude'
  printf 'This permanently deletes %s message(s) across %s folder(s).\n' "${grand_total}" "${folder_count}" >&2
  printf 'Type that message count (%s) to proceed, anything else to abort: ' "${grand_total}" >&2
  IFS= read -r typed || die 1 'stdin closed before the magnitude checkpoint — nothing expunged'
  [[ "${typed}" == "${grand_total}" ]] \
    || die 1 'checkpoint did not match the message count — nothing expunged'
}

# Expunges every ready pair, continuing past failures: one account's IMAP
# hiccup must not strand the other accounts' already-confirmed expunges. Each
# outcome rewrites the pair's row, so the final report is the single source of
# what actually happened.
sweep_expunge_ready() {
  local i status account_ folder_ count _detail rc
  local -a ids
  for i in "${!SWEEP_ROWS[@]}"; do
    IFS=$'\t' read -r status account_ folder_ count _detail <<<"${SWEEP_ROWS[i]}"
    [[ "${status}" == 'ready' ]] || continue

    read -ra ids <<<"${SWEEP_IDS["${account_}"$'\t'"${folder_}"]}"
    step "[${account_}] ${folder_}: expunging ${count} message(s)…"

    rc=0
    expunge_ids "${account_}" "${folder_}" "${ids[@]}" || rc=$?
    case "${rc}" in
      0)
        SWEEP_ROWS[i]=$(printf 'expunged\t%s\t%s\t%s\t' "${account_}" "${folder_}" "${count}")
        ;;
      1)
        SWEEP_ROWS[i]=$(printf 'finding\t%s\t%s\t%s\t%s' "${account_}" "${folder_}" 0 \
          'flagging failed — nothing was deleted in this folder')
        ;;
      *)
        SWEEP_ROWS[i]=$(printf 'finding\t%s\t%s\t%s\t%s' "${account_}" "${folder_}" 0 \
          'flagged but the expunge failed — the folder holds deleted-flagged mail; a re-run refuses it until cleared')
        ;;
    esac
  done
}

# The Expunge Sweep: host gates once, every pair probed, one summary, two
# checkpoints, then the expunges. Exits 1 when any row is a finding — the
# report needs reading — and 0 on a clean sweep.
run_sweep() {
  gate_backup_verified
  gate_archive_fresh

  local himalaya_names
  himalaya_names=$(himalaya_account_names) \
    || die 2 'himalaya account list returned JSON this script cannot read'

  # check_requested_accounts has already proved each name is a himalaya
  # account, so every entry here classifies into a row of its own.
  local requested=''
  ((${#requested_accounts[@]} == 0)) \
    || requested=$(printf '%s\n' "${requested_accounts[@]}")

  local class name
  while IFS=$'\t' read -r class name; do
    [[ -n "${class}" ]] || continue
    case "${class}" in
      eligible)
        account="${name}"
        sweep_probe_account
        ;;
      no-archive)
        sweep_record skip "${name}" '-' 0 "no Email Archive directory on ${host}"
        ;;
      no-account)
        sweep_record skip "${name}" '-' 0 'an archive directory no himalaya account matches'
        ;;
    esac
  done < <(sweep_classify_accounts "${himalaya_names}" "${ARCHIVE_ACCOUNTS}" "${requested}")

  if ((SWEEP_PAIRS_PROBED == 0)); then
    sweep_render_report 'Sweep classification'
    die 2 "no eligible (account, Synced Folder) pair in scope on ${host}" \
      'the rows above name why each candidate was skipped'
  fi

  sweep_print_summary

  local findings ready_count
  findings=$(sweep_rows_with_status finding | grep -c . || true)
  ready_count=$(sweep_rows_with_status ready | grep -c . || true)

  if ! interactive; then
    note ''
    note 'Every gate ran and every pair is classified above. The expunge needs'
    note 'an interactive TTY and two typed checkpoints; there is no unattended'
    note 'path (ADR-0007). Re-run this script from a terminal to expunge.'
    printf 'verified: %s pair(s) ready, %s finding(s) on %s\n' \
      "${ready_count}" "${findings}" "${host}"
    ((findings == 0)) || exit 1
    return 0
  fi

  if ((ready_count == 0)); then
    note ''
    note 'no pair is ready to expunge'
    printf 'swept: 0 message(s) expunged, %s finding(s) on %s\n' "${findings}" "${host}"
    ((findings == 0)) || exit 1
    return 0
  fi

  sweep_confirm_or_abort
  sweep_expunge_ready
  sweep_render_report 'Sweep report'

  local expunged_msgs expunged_folders
  findings=$(sweep_rows_with_status finding | grep -c . || true)
  expunged_msgs=$(sweep_rows_with_status expunged | sweep_sum_counts)
  expunged_folders=$(sweep_rows_with_status expunged | grep -c . || true)
  printf 'swept: %s message(s) expunged across %s folder(s), %s finding(s) on %s\n' \
    "${expunged_msgs}" "${expunged_folders}" "${findings}" "${host}"
  ((findings == 0)) || exit 1
}

main() {
  parse_args "$@"

  # Resolve one value at a time, cheapest check first, and validate each before
  # it is used to reach the next. Nothing prompts until the tools behind the
  # menus are known to work, and the account menu can only be built once the
  # Host has answered with its Email Archive contents.
  validate_options
  check_tools
  check_requested_accounts
  resolve_host
  validate_host
  check_host_reachable
  check_archive_root

  # The sweep departs here: everything above is host-scoped, everything below
  # resolves the one (account, folder) pair a single-target run works on.
  if [[ "${sweep}" == true ]]; then
    run_sweep
    return 0
  fi

  resolve_account
  validate_account
  check_account_archive
  list_account_folders
  list_synced_folders
  resolve_folder
  check_folder_exists
  check_folder_synced

  gate_backup_verified
  gate_archive_fresh
  gate_folder_coverage
  print_summary

  if ! interactive; then
    note ''
    note 'All gates passed. The expunge needs an interactive TTY and a typed'
    note 'confirmation; there is no unattended path (ADR-0007). Re-run this'
    note 'script from a terminal to expunge.'
    printf 'verified: %s message(s) eligible in %s\n' "${IMAP_COUNT}" "${folder}"
    return 0
  fi

  confirm_or_abort
  expunge
  printf 'expunged: %s message(s) from %s\n' "${IMAP_COUNT}" "${folder}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
