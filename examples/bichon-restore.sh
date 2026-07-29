#!/usr/bin/env bash
#
# examples/bichon-restore.sh
#
# REFERENCE SCRIPT — NOT SHIPPED IN THE auberge BINARY
# ----------------------------------------------------
# Restores one account's Email Archive into a running Bichon (ADR-0012):
#
#   1. group every archived .eml by the folder its .meta.json sidecar records
#   2. POST /api/v1/import per folder, in batches
#   3. replay the Tag Snapshot (tags.json): resolve each Message-ID to the
#      envelope ids the import just minted, then POST /api/v1/update-tags
#
# Non-interactive by design: restore is additive (it creates messages and adds
# tags, it deletes nothing), so it carries none of bichon-expunge.sh's TTY
# gates. What it does share: fail loudly, report exact counts, and refuse to
# guess when the archive or the API disagrees with expectations.
#
# Bichon's import mints a fresh envelope id per message, so re-running a
# completed restore duplicates messages. Tag replay alone is safe to re-run.
#
# Prerequisites:
#   - a running Bichon with the account already created (Accounts → Add
#     account in the UI) and its folders synced — Bichon only imports into
#     folders it already knows (auberge bichon reconcile-folders)
#   - the Email Archive readable at --archive-path (restored from restic
#     first, if this is disaster recovery)
#   - BICHON_API_TOKEN exported; on the Bichon Host it lives in
#     /etc/default/bichon-archive
#   - curl, jq, base64
#
# Exit codes:
#   0 — restored (or --dry-run plan printed)
#   1 — restore ran but something failed or could not be placed
#   2 — usage error or missing prerequisite
#
# shellcheck shell=bash

set -euo pipefail

readonly PROGRAM_NAME="${0##*/}"
readonly DEFAULT_ARCHIVE_PATH='/var/lib/bichon-archive'
# The batch size bichonctl's own EML import sends per /api/v1/import call.
readonly BATCH_SIZE=50
# /api/v1/search-messages caps page_size at 500 (EmailSearchRequest::validate).
readonly PAGE_SIZE=500

host=''
account=''
archive_path="${DEFAULT_ARCHIVE_PATH}"
dry_run=false

ACCOUNT_ID=''
ACCOUNT_DIR=''
TAGS_FILE=''
WORK_DIR=''
declare -A FOLDER_FILES=()
TOTAL_EML=0
IMPORTED=0
TAGGED=0
SKIPPED=0
FAILED=0

usage() {
  cat <<EOF
Restore an account's Email Archive into a running Bichon: import every .eml
into the folder its sidecar records, then replay the Tag Snapshot.

Usage: ${PROGRAM_NAME} --host URL --account ADDRESS [options]

Options:
  -H, --host URL             base URL of the Bichon REST API
                             (http://127.0.0.1:15630 on the Bichon Host, or
                             the https URL from the operator's tailnet)
  -a, --account ADDRESS      mailbox email address; names the Email Archive
                             directory and must match a Bichon account
      --archive-path PATH    Email Archive root
                             (default: ${DEFAULT_ARCHIVE_PATH})
      --dry-run              print the folder and tag-set plan; no API calls
  -h, --help                 show this help

BICHON_API_TOKEN must be exported for anything beyond --dry-run.

The account and its folders must already exist in Bichon: restore imports
messages, it does not create accounts or folders.
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
      --archive-path)
        [[ $# -ge 2 ]] || die 2 "$1 needs a value"
        archive_path="$2"
        shift 2
        ;;
      --dry-run)
        dry_run=true
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

validate_options() {
  [[ "${account}" == *[![:space:]]* ]] \
    || die 2 'missing --account' \
      'a non-interactive script must be told which mailbox to restore'

  [[ "${archive_path}" == /* ]] \
    || die 2 "archive-path must be absolute, got: ${archive_path}"

  if [[ "${dry_run}" == false ]]; then
    [[ "${host}" =~ ^https?:// ]] \
      || die 2 "not a Bichon API base URL: ${host:-<missing --host>}" \
        'expected http(s)://…, e.g. http://127.0.0.1:15630 on the Bichon Host'
    host="${host%/}"

    [[ -n "${BICHON_API_TOKEN:-}" ]] \
      || die 2 'BICHON_API_TOKEN is not set' \
        'export the token the archive timer uses; on the Bichon Host it' \
        'lives in /etc/default/bichon-archive'
  fi
}

check_tools() {
  local tool missing=()
  for tool in curl jq base64; do
    command -v "${tool}" >/dev/null 2>&1 || missing+=("${tool}")
  done
  ((${#missing[@]} == 0)) \
    || die 2 "not on PATH: ${missing[*]}"
}

# bichon names each Email Archive directory after the mailbox email address,
# with '/' replaced. Keep in step with sanitize_email in
# ansible/roles/bichon/templates/bichon-archive.sh.j2.
archive_dir_for() {
  printf '%s' "${1//\//_}"
}

# GNU base64 wraps at 76 columns unless told not to; BSD/macOS base64 does not
# wrap and has no -w. Detect once, not per message.
B64_FLAGS=()
detect_base64() {
  if base64 -w0 </dev/null >/dev/null 2>&1; then
    B64_FLAGS=(-w0)
  fi
}

curl_auth() {
  curl --silent --show-error --fail \
    --retry 5 --retry-all-errors --retry-delay 10 --retry-max-time 300 \
    --header "Authorization: Bearer ${BICHON_API_TOKEN}" \
    "$@"
}

check_archive() {
  ACCOUNT_DIR="${archive_path}/$(archive_dir_for "${account}")"
  TAGS_FILE="${ACCOUNT_DIR}/tags.json"

  [[ -d "${ACCOUNT_DIR}" ]] \
    || die 2 "no Email Archive directory for ${account}" \
      "expected: ${ACCOUNT_DIR}" \
      '--account must be the mailbox email address that names the archive' \
      'directory; restore the archive from restic first if it is not local'
}

build_plan() {
  step "Reading the Email Archive at ${ACCOUNT_DIR}…"

  local emls=()
  mapfile -t emls < <(find "${ACCOUNT_DIR}" -type f -name '*.eml' | sort)
  ((${#emls[@]} > 0)) \
    || die 1 "no .eml files under ${ACCOUNT_DIR}" \
      'nothing to restore — check --archive-path and --account'

  local eml sidecar folder
  for eml in "${emls[@]}"; do
    sidecar="${eml%.eml}.meta.json"
    if [[ ! -f "${sidecar}" ]]; then
      note "no sidecar for ${eml}; cannot place it in a folder"
      FAILED=$((FAILED + 1))
      continue
    fi
    # Sidecars written before ADR-0012 also carry a vestigial `tags` field;
    # folder is the only fact read here, tags come from tags.json alone.
    folder=$(jq -r '.folder // empty' "${sidecar}") || folder=''
    if [[ -z "${folder}" || "${folder}" == *$'\n'* ]]; then
      note "unusable folder in ${sidecar}"
      FAILED=$((FAILED + 1))
      continue
    fi
    FOLDER_FILES[${folder}]+="${eml}"$'\n'
    TOTAL_EML=$((TOTAL_EML + 1))
  done

  ((TOTAL_EML > 0)) \
    || die 1 "no importable .eml files under ${ACCOUNT_DIR}" \
      'every message is missing its sidecar or a usable folder value'
}

print_plan() {
  step 'Plan'

  local folders=() folder count
  mapfile -t folders < <(printf '%s\n' "${!FOLDER_FILES[@]}" | sort)
  for folder in "${folders[@]}"; do
    count=$(printf '%s' "${FOLDER_FILES[${folder}]}" | wc -l)
    printf '  %-40s %6s message(s)\n' "${folder}" "${count}" >&2
  done

  note ''
  if [[ ! -f "${TAGS_FILE}" ]]; then
    note 'no tags.json in the account archive; no tags to replay'
    return 0
  fi

  local sets
  sets=$(jq -r '
    to_entries
    | map(.value |= sort)
    | group_by(.value)
    | map("  [\(.[0].value | join(", "))]  \(length) message-id(s)")
    | .[]' "${TAGS_FILE}") \
    || die 1 "cannot read ${TAGS_FILE}" \
      'the Tag Snapshot is written atomically by bichon-archive; a corrupt' \
      'one means the archive copy is damaged — restore it from restic'

  if [[ -n "${sets}" ]]; then
    note 'tag sets to replay (from tags.json):'
    printf '%s\n' "${sets}" >&2
  else
    note 'tags.json holds no entries; no tags to replay'
  fi
}

verify_auth() {
  step "Checking the Bichon API at ${host}…"
  curl_auth "${host}/api/v1/current-user" >/dev/null \
    || die 2 "cannot authenticate against ${host}" \
      'check --host and BICHON_API_TOKEN'
  note 'auth ok'
}

resolve_account_id() {
  step "Resolving ${account} in Bichon…"
  local accounts
  accounts=$(curl_auth "${host}/api/v1/minimal-account-list?only_nosync=false") \
    || die 2 'minimal-account-list failed'
  ACCOUNT_ID=$(printf '%s' "${accounts}" | jq -r --arg email "${account}" \
    '.[] | select(.email == $email) | .id')
  [[ -n "${ACCOUNT_ID}" ]] \
    || die 1 "Bichon has no account for ${account}" \
      'restore does not create accounts — add it in the Bichon UI first' \
      '(Accounts → Add account), let it sync, then re-run'
  note "account id ${ACCOUNT_ID}"
}

import_batch() {
  local folder="$1"
  shift
  local b64_file="${WORK_DIR}/batch.b64" payload_file="${WORK_DIR}/batch.json"
  local f b64
  : >"${b64_file}"
  for f in "$@"; do
    # URL-safe base64 with padding — the server decodes with base64's
    # URL_SAFE engine (base64_decode_url_safe!).
    if ! b64=$(base64 "${B64_FLAGS[@]}" <"${f}" | tr '+/' '-_'); then
      note "cannot read ${f}"
      return 1
    fi
    printf '%s\n' "${b64}" >>"${b64_file}"
  done

  jq -Rn --argjson account_id "${ACCOUNT_ID}" --arg mail_folder "${folder}" \
    '{account_id: $account_id, mail_folder: $mail_folder, emls: [inputs]}' \
    <"${b64_file}" >"${payload_file}"

  local body
  body=$(curl_auth --header 'Content-Type: application/json' --request POST \
    --data @"${payload_file}" "${host}/api/v1/import") || return 1

  local ok bad
  ok=$(printf '%s' "${body}" | jq -r '.success')
  bad=$(printf '%s' "${body}" | jq -r '.failed')
  IMPORTED=$((IMPORTED + ok))
  FAILED=$((FAILED + bad))
  if ((bad > 0)); then
    printf '%s' "${body}" | jq -r \
      '.failed_details[] | "  import error at index \(.index): \(.error_message)"' >&2
  fi
}

import_folder() {
  local folder="$1"
  local files=()
  mapfile -t files < <(printf '%s' "${FOLDER_FILES[${folder}]}")

  note "importing ${#files[@]} message(s) into ${folder}…"

  local done_count=0
  while ((done_count < ${#files[@]})); do
    local batch=("${files[@]:done_count:BATCH_SIZE}")
    if ! import_batch "${folder}" "${batch[@]}"; then
      local remaining=$((${#files[@]} - done_count))
      FAILED=$((FAILED + remaining))
      note "import failed for ${folder}; skipping its remaining ${remaining} message(s)"
      note "Bichon only imports into folders it already knows — check the"
      note "folder exists and is synced (auberge bichon reconcile-folders)"
      return 0
    fi
    done_count=$((done_count + ${#batch[@]}))
  done
}

run_import() {
  step "Importing ${TOTAL_EML} message(s) into ${#FOLDER_FILES[@]} folder(s)…"
  local folders=() folder
  mapfile -t folders < <(printf '%s\n' "${!FOLDER_FILES[@]}" | sort)
  for folder in "${folders[@]}"; do
    import_folder "${folder}"
  done
}

# Emit every envelope id whose message_id matches, across pages. Import mints
# fresh envelope ids, so the Snapshot's Message-ID keys are the only stable
# join between the archive and the corpus Bichon now holds.
resolve_envelope_ids() {
  local mid="$1"
  local page=1 total_pages=1 payload body
  while ((page <= total_pages)); do
    payload=$(jq -nc \
      --arg mid "${mid}" \
      --argjson account_id "${ACCOUNT_ID}" \
      --argjson page "${page}" \
      --argjson page_size "${PAGE_SIZE}" \
      '{filter: {message_id: $mid, account_ids: [$account_id]}, page: $page, page_size: $page_size, sort_by: "DATE", desc: false}')
    body=$(curl_auth --header 'Content-Type: application/json' --request POST \
      --data "${payload}" "${host}/api/v1/search-messages") || return 1
    printf '%s' "${body}" | jq -r '.items[]?.id'
    total_pages=$(printf '%s' "${body}" | jq -r '.total_pages // 0')
    page=$((page + 1))
  done
}

run_tag_replay() {
  if [[ ! -f "${TAGS_FILE}" ]]; then
    note 'no tags.json in the account archive; skipping tag replay'
    return 0
  fi

  local entry_count
  entry_count=$(jq 'length' "${TAGS_FILE}")
  if ((entry_count == 0)); then
    note 'tags.json is empty; nothing to replay'
    return 0
  fi

  step "Replaying tags for ${entry_count} message-id(s)…"

  # One update-tags call applies one tag set to many envelopes, so group
  # envelope ids by identical (sorted) tag set before calling.
  local -A tagset_ids=()
  local entry mid tagset ids
  while IFS= read -r entry; do
    mid=$(printf '%s' "${entry}" | jq -r '.key')
    tagset=$(printf '%s' "${entry}" | jq -c '.value | sort')
    if ! ids=$(resolve_envelope_ids "${mid}"); then
      note "search-messages failed for message-id ${mid}"
      FAILED=$((FAILED + 1))
      continue
    fi
    if [[ -z "${ids}" ]]; then
      # Known gap (ADR-0012): messages that arrived without a Message-ID got
      # a synthetic one that is not stable across re-import. Their tags are
      # lost; the messages themselves are already restored.
      note "no envelope for message-id ${mid}; its tags cannot be replayed"
      SKIPPED=$((SKIPPED + 1))
      continue
    fi
    tagset_ids[${tagset}]+="${ids}"$'\n'
  done < <(jq -c 'to_entries[]' "${TAGS_FILE}")

  local ids_json payload
  for tagset in "${!tagset_ids[@]}"; do
    ids_json=$(printf '%s' "${tagset_ids[${tagset}]}" \
      | jq -Rn '[inputs | select(length > 0)]')
    # action=Add rather than Overwrite: a partial re-run only ever adds, so
    # replaying over an already-tagged corpus is non-destructive.
    payload=$(jq -nc \
      --arg account_id "${ACCOUNT_ID}" \
      --argjson tags "${tagset}" \
      --argjson ids "${ids_json}" \
      '{updates: {($account_id): $ids}, tags: $tags, action: "Add"}')
    if curl_auth --header 'Content-Type: application/json' --request POST \
      --data "${payload}" "${host}/api/v1/update-tags" >/dev/null; then
      TAGGED=$((TAGGED + $(printf '%s' "${ids_json}" | jq 'length')))
    else
      note "update-tags failed for tag set ${tagset}"
      FAILED=$((FAILED + 1))
    fi
  done
}

main() {
  parse_args "$@"
  validate_options
  check_tools
  detect_base64
  check_archive
  build_plan
  print_plan

  if [[ "${dry_run}" == true ]]; then
    printf 'dry run: %s message(s) in %s folder(s), %s unplaceable, nothing imported\n' \
      "${TOTAL_EML}" "${#FOLDER_FILES[@]}" "${FAILED}"
    ((FAILED == 0)) || exit 1
    return 0
  fi

  WORK_DIR=$(mktemp -d)
  trap 'rm -rf "${WORK_DIR}"' EXIT

  verify_auth
  resolve_account_id
  run_import
  run_tag_replay

  printf 'imported: %s message(s), tagged: %s envelope(s), skipped: %s message-id(s), failed: %s\n' \
    "${IMPORTED}" "${TAGGED}" "${SKIPPED}" "${FAILED}"
  ((FAILED == 0)) || exit 1
}

main "$@"
