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
#   3. folder coverage    archive .eml count >= IMAP count, scoped to --folder
#   4. summary            exact commands, message count, snapshot evidence
#   5. typed confirmation operator types the folder name on a TTY
#
# Safety contract (ADR-0007) — load-bearing, do not "improve" away:
#   - stdin must be a TTY; a non-TTY run refuses the expunge unconditionally
#   - there is no --yes / --force, so no unattended expunge path exists
#   - the typed folder name is an intent checksum against a mangled --folder
#
# Designed for MXroute; works with any IMAP provider himalaya supports.
#
# Prerequisites:
#   - auberge with the `backup verify` subcommand (>= the release carrying it)
#   - himalaya  (https://github.com/pimalaya/himalaya — Rust, project ethos)
#   - jq
#   - key-based ssh access to the Bichon Host, with journal/systemctl read
#
# Exit codes:
#   0 — expunged, or (non-interactive) all gates passed and expunge skipped
#   1 — a gate failed, or the typed confirmation did not match
#   2 — usage error, missing prerequisite, or unreachable Host
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
account=''
folder="${DEFAULT_FOLDER}"
window_days="${DEFAULT_WINDOW_DAYS}"
archive_path="${DEFAULT_ARCHIVE_PATH}"
no_input=false

# Gate findings, threaded from the gate that computes them to the summary and
# the expunge.
BACKUP_EVIDENCE=''
CUTOFF_DATE=''
ENVELOPE_JSON=''
IMAP_COUNT=0

usage() {
  cat <<EOF
Expunge archived mail from an Upstream Mailbox, behind five safety gates.

Usage: ${PROGRAM_NAME} [options]

Options:
  -H, --host HOST            ssh target of the Bichon Host, also passed to
                             \`auberge backup verify --host\` (prompted if a TTY)
  -a, --account ADDRESS      himalaya account to expunge from (prompted if a TTY)
  -f, --folder NAME          folder to expunge (default: ${DEFAULT_FOLDER})
  -w, --window-days DAYS     expunge mail older than DAYS (default: ${DEFAULT_WINDOW_DAYS})
      --archive-path PATH    Email Archive root on the Host
                             (default: ${DEFAULT_ARCHIVE_PATH})
      --no-input             never prompt; requires --host and --account, and
                             refuses the expunge (gates still run)
  -h, --help                 show this help

The expunge requires an interactive TTY. There is deliberately no flag that
skips the typed confirmation.
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
        account="$2"
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

# Read one value from the operator. Prompt on stderr so stdout stays clean.
prompt_for() {
  local label="$1" value
  printf '%s: ' "${label}" >&2
  IFS= read -r value || die 2 "stdin closed while reading ${label}"
  printf '%s' "${value}"
}

resolve_missing_flags() {
  if [[ -z "${host}" ]]; then
    interactive || die 2 'missing --host' \
      'a non-interactive run must pass every value as a flag'
    host=$(prompt_for 'Bichon Host (ssh target)')
  fi

  if [[ -z "${account}" ]]; then
    interactive || die 2 'missing --account' \
      'a non-interactive run must pass every value as a flag'
    account=$(prompt_for 'himalaya account (email address)')
  fi
}

validate_flags() {
  # A host starting with '-' would be parsed by ssh as an option, so the
  # character class is a guard, not cosmetics.
  [[ "${host}" =~ ^[A-Za-z0-9][A-Za-z0-9._@-]*$ ]] \
    || die 2 "not a usable ssh target: ${host}" \
      'expected a hostname, ssh_config alias, or user@host'

  [[ "${account}" == *[![:space:]]* ]] \
    || die 2 'account must not be empty'

  [[ "${folder}" == *[![:space:]]* ]] \
    || die 2 'folder must not be empty'

  # A newline in the folder name would desynchronise the typed-confirmation
  # comparison from what the operator sees on screen.
  [[ "${folder}" != *$'\n'* ]] \
    || die 2 'folder must not contain a newline'

  [[ "${window_days}" =~ ^[1-9][0-9]*$ ]] \
    || die 2 "window-days must be a positive integer, got: ${window_days}"

  [[ "${archive_path}" == /* ]] \
    || die 2 "archive-path must be absolute, got: ${archive_path}"
}

check_prerequisites() {
  step 'Checking prerequisites…'

  local tool
  for tool in auberge himalaya jq ssh; do
    command -v "${tool}" >/dev/null 2>&1 \
      || die 2 "${tool} is not on PATH" \
        'install it before running this script'
  done

  # Gate 1 needs `auberge backup verify`. clap exits non-zero on an unknown
  # subcommand, which is exactly the signal for "your auberge is too old".
  auberge backup verify --help >/dev/null 2>&1 \
    || die 2 'this auberge has no "backup verify" subcommand' \
      'upgrade auberge: mise up auberge' \
      'gate 1 asserts the archive reached the off-host restic repository;' \
      'there is no weaker substitute for it'

  ssh "${SSH_OPTS[@]}" "${host}" true 2>/dev/null \
    || die 2 "cannot ssh to ${host}" \
      "check: ssh ${host}" \
      'key-based auth is required (this script runs ssh in BatchMode)'

  note 'auberge, himalaya, jq, ssh ok'
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
  step 'Gate 1/5 — off-host backup…'

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
  step 'Gate 2/5 — archive freshness…'

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

# Gate 3 — every in-window IMAP message has an archived counterpart.
# Sets IMAP_COUNT and ENVELOPE_JSON; the expunge reuses the same envelope set,
# so the messages counted here are exactly the messages deleted later.
gate_folder_coverage() {
  step "Gate 3/5 — coverage for ${folder} older than ${window_days} days…"

  local cutoff_date cutoff_ym
  cutoff_date=$(date_offset "${window_days}" '+%Y-%m-%d')
  cutoff_ym=$(date_offset "${window_days}" '+%Y/%m')
  CUTOFF_DATE="${cutoff_date}"

  # --output json: the default table format adds headers and box drawing, so
  # `wc -l` is off by a few. jq counts envelopes precisely. The filter query is
  # a trailing positional in himalaya, so it must come after every flag.
  ENVELOPE_JSON=$(himalaya --output json envelope list \
    --account "${account}" \
    --folder "${folder}" \
    --page-size "${PAGE_SIZE}" \
    before "${cutoff_date}" 2>/dev/null) \
    || die 1 "himalaya could not list ${folder} for ${account}" \
      "check the account exists: himalaya account list" \
      "check the folder exists: himalaya folder list --account ${account}"

  IMAP_COUNT=$(printf '%s' "${ENVELOPE_JSON}" | jq 'length')
  note "IMAP messages in window: ${IMAP_COUNT}"

  ((IMAP_COUNT > 0)) \
    || die 1 "no IMAP messages in ${folder} older than ${window_days} days" \
      'nothing to expunge — check --folder and --account'

  # `folder expunge` removes every \Deleted-flagged message in the folder, not
  # only the ones this script flags. A pre-existing flag would therefore be
  # collateral damage, so refuse rather than widen the blast radius.
  local already_deleted
  already_deleted=$(himalaya --output json envelope list \
    --account "${account}" \
    --folder "${folder}" \
    --page-size "${PAGE_SIZE}" \
    flag deleted 2>/dev/null | jq 'length')

  ((already_deleted == 0)) \
    || die 1 "${already_deleted} message(s) in ${folder} already carry the deleted flag" \
      "inspect them: himalaya envelope list --account ${account} --folder ${folder} flag deleted" \
      'clear or expunge them separately — this script only expunges what it flags'

  # Archive paths encode the message Date as YYYY/MM; folder identity lives in
  # the <id>.meta.json sidecar. The IMAP query is folder-scoped, so the archive
  # count must be too — counting account-wide .eml files would over-count and
  # pass this gate while the target folder is only partially archived.
  local safe_account archive_dir eml_count
  safe_account=$(printf '%s' "${account}" | tr '/' '_')
  archive_dir="${archive_path}/${safe_account}"

  # Remote args go through printf %q so ${folder} / ${archive_dir} cannot break
  # out of argument quoting and execute on the Host. The heredoc body is
  # single-quoted ('REMOTE') to disable local expansion — only positional
  # parameters are used.
  # shellcheck disable=SC2029  # intentional: %q-quote args locally, then expand into ssh cmd
  eml_count=$(
    ssh "${SSH_OPTS[@]}" "${host}" \
      "bash -s -- $(printf '%q ' "${archive_dir}" "${folder}" "${cutoff_ym}")" <<'REMOTE'
set -euo pipefail
archive_dir=$1; folder=$2; cutoff_ym=$3
find "$archive_dir" -regextype posix-extended \
  -regex '.*/[0-9]{4}/[0-9]{2}/[^/]+\.meta\.json' 2>/dev/null \
  | while IFS= read -r meta; do
    ym=$(printf '%s' "$meta" | awk -F/ '{ print $(NF-2)"/"$(NF-1) }')
    [ "$ym" \> "$cutoff_ym" ] && continue
    jq -e --arg f "$folder" '.folder == $f' "$meta" >/dev/null 2>&1 && printf '.\n'
  done | wc -l
REMOTE
  )
  note "archive .eml files in window: ${eml_count}"

  ((eml_count >= IMAP_COUNT)) \
    || die 1 "coverage gap: archive has ${eml_count} files, IMAP has ${IMAP_COUNT} messages" \
      "read the journal: ssh ${host} journalctl -u bichon-archive.service" \
      "check ${folder} is a Synced Folder: auberge bichon reconcile-folders --host ${host}" \
      'do not expunge until the counts reconcile'

  note "coverage ok (${eml_count} >= ${IMAP_COUNT})"
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

expunge() {
  local ids=()
  mapfile -t ids < <(printf '%s' "${ENVELOPE_JSON}" | jq -r '.[].id')

  ((${#ids[@]} == IMAP_COUNT)) \
    || die 1 "envelope ids (${#ids[@]}) do not match the counted messages (${IMAP_COUNT})" \
      'refusing to act on a listing that changed underfoot'

  # himalaya reads any non-numeric positional as a flag name, so a stray value
  # here would silently become a flag instead of a uid.
  local id
  for id in "${ids[@]}"; do
    [[ "${id}" =~ ^[0-9]+$ ]] \
      || die 1 "himalaya returned a non-numeric envelope id: ${id}"
  done

  step "Flagging ${#ids[@]} message(s) as deleted…"
  himalaya flag add \
    --account "${account}" \
    --folder "${folder}" \
    "${ids[@]}" deleted >&2

  step "Expunging ${folder}…"
  himalaya folder expunge --account "${account}" "${folder}" >&2
}

main() {
  parse_args "$@"
  resolve_missing_flags
  validate_flags

  check_prerequisites
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

main "$@"
