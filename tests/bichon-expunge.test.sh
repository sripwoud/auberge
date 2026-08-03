#!/usr/bin/env bash
#
# tests/bichon-expunge.test.sh
#
# Unit tests for the pure, network-free logic in examples/bichon-expunge.sh —
# the last checks between an operator and permanently deleting mail from an
# Upstream Mailbox. Covers gate 3's coverage verdict (deduplication and the
# refusal to guess), the case-insensitive folder-name near-miss, the Synced
# Folder eligibility verdict, and the --folder default/passthrough, all
# against in-memory input: no Host, no archive, no himalaya, no IMAP, no
# auberge.
#
# The Host-side emitter that produces gate 3's rows is not unit-tested here; it
# runs on the Bichon Host and is exercised end to end by a `--no-input` run.
# list_synced_folders is untested for the same reason — it shells out to
# `auberge bichon reconcile-folders` — but check_folder_synced, which consumes
# its output, needs no network and is covered below.
#
# Run: ./tests/bichon-expunge.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"
# shellcheck source=../examples/bichon-expunge.sh disable=SC1091
source "${REPO_ROOT}/examples/bichon-expunge.sh"

# Render the verdict for a row set: the distinct count, or which sidecar made the
# count unknowable.
verdict_for() {
  if count_distinct_message_ids <<<"$1"; then
    printf '%s' "${ARCHIVE_COUNT}"
  else
    printf 'unkeyed:%s' "${UNKEYED_SIDECAR}"
  fi
}

printf '== count_distinct_message_ids\n'

# The defect this gate had: five sidecars, three messages. Counting files
# credited the archive with five and let the gate pass on evidence for three.
assert_eq 'five sidecars holding three messages count as three' '3' \
  "$(verdict_for '/a/2026/07/1.meta.json	a@example.com
/a/2026/07/2.meta.json	a@example.com
/a/2026/07/3.meta.json	b@example.com
/a/2026/07/4.meta.json	b@example.com
/a/2026/07/5.meta.json	c@example.com')"

assert_eq 'one sidecar counts as one message' '1' \
  "$(verdict_for '/a/2026/07/1.meta.json	a@example.com')"

assert_eq 'an empty row set is zero coverage' '0' "$(verdict_for '')"

assert_eq 'blank rows are not coverage' '0' "$(verdict_for '

')"

assert_eq 'hash-keyed sidecars deduplicate like any other id' '2' \
  "$(verdict_for '/a/2026/07/1.meta.json	sha256:aa
/a/2026/07/2.meta.json	sha256:aa
/a/2026/07/3.meta.json	sha256:bb')"

assert_eq 'ids differing only in case are different messages' '2' \
  "$(verdict_for '/a/2026/07/1.meta.json	A@example.com
/a/2026/07/2.meta.json	a@example.com')"

# The field is tab-delimited precisely so an id is taken whole; a value split on
# spaces would key two sidecars of one message differently.
assert_eq 'an id containing spaces is taken whole' '1' \
  "$(verdict_for '/a/2026/07/1.meta.json	odd id@example.com
/a/2026/07/2.meta.json	odd id@example.com')"

printf '\n== refusal on a sidecar predating Message-ID keying\n'

assert_eq 'a sidecar with no message_id aborts and is named' \
  'unkeyed:/a/2026/07/2.meta.json' \
  "$(verdict_for '/a/2026/07/1.meta.json	a@example.com
/a/2026/07/2.meta.json
/a/2026/07/3.meta.json	c@example.com')"

assert_eq 'a row with no tab at all aborts rather than counting the path' \
  'unkeyed:/a/2026/07/1.meta.json' \
  "$(verdict_for '/a/2026/07/1.meta.json')"

assert_eq 'the first unkeyed sidecar is the one reported' \
  'unkeyed:/a/2026/07/1.meta.json' \
  "$(verdict_for '/a/2026/07/1.meta.json
/a/2026/07/2.meta.json	')"

assert_fails 'an unkeyed sidecar returns non-zero' \
  count_distinct_message_ids <<<'/a/2026/07/1.meta.json	'

# The gate calls this twice per run in the worst case; a stale path or count from
# an earlier call would misreport which sidecar to repair.
count_distinct_message_ids <<<'/a/2026/07/1.meta.json	' || true
assert_eq 'a later clean call clears the earlier abort' '1' \
  "$(verdict_for '/a/2026/07/9.meta.json	z@example.com')"

count_distinct_message_ids <<<'/a/2026/07/9.meta.json	z@example.com'
assert_eq 'a clean call leaves no sidecar named' '' "${UNKEYED_SIDECAR}"

printf '\n== folder_case_insensitive_match\n'

assert_eq 'a case mangled folder finds its advertised casing' 'Sent' \
  "$(folder_case_insensitive_match SENT <<<$'Archive\nINBOX\nSent\nTrash')"

assert_eq 'an exact name matches itself' 'INBOX' \
  "$(folder_case_insensitive_match INBOX <<<$'Archive\nINBOX\nSent\nTrash')"

assert_fails 'a substring is not a match' \
  folder_case_insensitive_match Sen <<<$'Archive\nINBOX\nSent\nTrash'

assert_fails 'no match returns non-zero' \
  folder_case_insensitive_match Trashed <<<$'Archive\nINBOX\nSent\nTrash'

assert_eq 'no match prints nothing' '' \
  "$(folder_case_insensitive_match Trashed <<<$'Archive\nINBOX\nSent\nTrash')"

printf '\n== resolve_folder\n'

# Forces the non-interactive branch: on a developer TTY, interactive() would
# otherwise open a real select menu and hang the suite.
# shellcheck disable=SC2034  # intentional: read by interactive() in the sourced script
no_input=true

folder=''
resolve_folder
assert_eq 'an omitted folder falls back to INBOX non-interactively' 'INBOX' "${folder}"

folder='Sent'
resolve_folder
assert_eq 'a passed folder is left untouched' 'Sent' "${folder}"

printf '\n== check_folder_synced\n'

# check_folder_synced calls die, which exits — run it in a subshell so that
# exit lands on the wrapper, not the suite. Functions are sourced, so a plain
# ( ... ) subshell still sees check_folder_synced.
synced_verdict() {
  (
    # shellcheck disable=SC2034  # intentional: read by check_folder_synced in the sourced script
    folder="$1" SYNCED_FOLDERS="$2" DRIFT_ADDED="$3" DRIFT_REMOVED="$4"
    # shellcheck disable=SC2034  # intentional: read by check_folder_synced in the sourced script
    account='a@example.com' host='h'
    check_folder_synced
  ) 2>&1
}

# Discards synced_verdict's output for exit-status-only assertions.
synced_ok() {
  synced_verdict "$1" "$2" "$3" "$4" >/dev/null
}

# True if $1 contains $2 — checks a die message names the right reason without
# pinning the whole remediation text.
contains() {
  [[ "$1" == *"$2"* ]]
}

SYNCED=$'INBOX\nSent'

assert_succeeds 'a synced folder is eligible' synced_ok Sent "${SYNCED}" '' ''
assert_fails 'a non-synced folder is refused' synced_ok Trash "${SYNCED}" '' ''

assert_succeeds 'the refusal names it as not a Synced Folder' \
  contains "$(synced_verdict Trash "${SYNCED}" '' '')" 'not a Synced Folder'

assert_succeeds 'a folder pending removal names the drift' \
  contains "$(synced_verdict Receipts "${SYNCED}" '' 'Receipts')" 'being removed'

assert_succeeds 'a folder pending addition names the drift' \
  contains "$(synced_verdict Newsletter "${SYNCED}" 'Newsletter' '')" 'not yet in the Synced Folder set'

# Sent is synced; the drift lists both name a different folder and must not
# override the eligibility this SYNCED_FOLDERS match already grants.
assert_succeeds 'eligibility beats drift lists for other folders' \
  synced_ok Sent "${SYNCED}" 'Newsletter' 'Receipts'

printf '\n== sweep_classify_accounts\n'

assert_eq 'accounts split into eligible, no-archive, and no-account' \
  $'eligible\ta@x.com\nno-archive\tb@x.com\nno-account\tstray@x.com' \
  "$(sweep_classify_accounts $'a@x.com\nb@x.com' $'a@x.com\nstray@x.com')"

# bichon writes the archive directory with '/' sanitized to '_'; the account
# name still has the slash, and the match must survive the difference.
assert_eq 'an account with a slash matches its sanitized directory' \
  $'eligible\ta/b@x.com' \
  "$(sweep_classify_accounts 'a/b@x.com' 'a_b@x.com')"

assert_eq 'no himalaya accounts leaves only orphan archive directories' \
  $'no-account\td@x.com' \
  "$(sweep_classify_accounts '' 'd@x.com')"

printf '\n== sweep_classify_folders\n'

SWEEP_SYNCED=$'Archive\nINBOX\nSent'
SWEEP_UPSTREAM=$'Drafts\nINBOX\nSent\nTrash'

assert_eq 'folders split into eligible, missing-upstream, and drift' \
  $'eligible\tINBOX\neligible\tSent\nmissing-upstream\tArchive\ndrift-added\tNewsletter\ndrift-removed\tReceipts' \
  "$(sweep_classify_folders "${SWEEP_SYNCED}" "${SWEEP_UPSTREAM}" 'Newsletter' 'Receipts')"

assert_eq 'no drift adds no drift rows' \
  $'eligible\tINBOX\neligible\tSent\nmissing-upstream\tArchive' \
  "$(sweep_classify_folders "${SWEEP_SYNCED}" "${SWEEP_UPSTREAM}" '' '')"

printf '\n== sweep rows: filtering, totals, rendering\n'

SWEEP_ROWS=(
  $'ready\ta@x.com\tINBOX\t12\t'
  $'skip\ta@x.com\tSent\t0\tnothing in window'
  $'finding\tb@x.com\tINBOX\t0\tcoverage gap: archive 3 < IMAP 5'
  $'ready\tb@x.com\tSent\t30\t'
)

assert_eq 'the grand total sums only the ready rows' '42' \
  "$(sweep_rows_with_status ready | sweep_sum_counts)"

assert_eq 'status filtering keeps whole rows' \
  $'ready\ta@x.com\tINBOX\t12\t\nready\tb@x.com\tSent\t30\t' \
  "$(sweep_rows_with_status ready)"

assert_eq 'a status no row holds filters to nothing' '' \
  "$(sweep_rows_with_status expunged)"

assert_eq 'an empty row set sums to zero' '0' \
  "$(printf '' | sweep_sum_counts)"

SWEEP_RENDERED=$(printf '%s\n' "${SWEEP_ROWS[@]}" | render_sweep_rows)

assert_succeeds 'a finding shouts in the report' \
  contains "${SWEEP_RENDERED}" 'FINDING'

assert_succeeds 'a finding carries its reason' \
  contains "${SWEEP_RENDERED}" 'coverage gap: archive 3 < IMAP 5'

assert_succeeds 'a ready row carries its count' \
  contains "${SWEEP_RENDERED}" '12'

printf '\n== collect_envelope_ids\n'

ENVELOPE_JSON='[{"id":"1"},{"id":"2"},{"id":"3"}]'
IMAP_COUNT=3
assert_succeeds 'a clean listing collects its ids' collect_envelope_ids
assert_eq 'the ids survive in order' '1 2 3' "${ENVELOPE_IDS[*]}"

# The listing gate 3 counted is the only thing the expunge may act on; a
# diverging id count means it changed underfoot.
IMAP_COUNT=2
assert_fails 'a count mismatch refuses the listing' collect_envelope_ids
assert_succeeds 'the mismatch is named' contains "${IDS_ERROR}" 'do not match'

# himalaya reads a non-numeric positional as a flag name, so one stray value
# would silently rewrite the command.
# shellcheck disable=SC2034  # intentional: read by collect_envelope_ids in the sourced script
ENVELOPE_JSON='[{"id":"1"},{"id":"x"}]' IMAP_COUNT=2
assert_fails 'a non-numeric id refuses the listing' collect_envelope_ids
assert_succeeds 'the offending id is named' contains "${IDS_ERROR}" 'non-numeric'

printf '\n== --sweep excludes --account/--folder\n'

# validate_options calls die, which exits — same subshell shape as
# synced_verdict above.
sweep_flags_ok() {
  (
    # shellcheck disable=SC2030,SC2034  # intentional: read by validate_options in the sourced script
    sweep=true account="$1" folder="$2" window_days=90 archive_path='/x'
    validate_options
  ) >/dev/null 2>&1
}

assert_fails '--sweep refuses a passed --account' sweep_flags_ok 'a@x.com' ''
assert_fails '--sweep refuses a passed --folder' sweep_flags_ok '' 'INBOX'
assert_succeeds '--sweep alone passes option validation' sweep_flags_ok '' ''

report 'bichon-expunge'
