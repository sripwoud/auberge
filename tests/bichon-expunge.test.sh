#!/usr/bin/env bash
#
# tests/bichon-expunge.test.sh
#
# Unit tests for the pure, network-free logic in examples/bichon-expunge.sh —
# the last checks between an operator and permanently deleting mail from an
# Upstream Mailbox. Covers gate 3's coverage verdict (deduplication and the
# refusal to guess), the case-insensitive folder-name near-miss, and the
# --folder default/passthrough, all against in-memory input: no Host, no
# archive, no himalaya, no IMAP.
#
# The Host-side emitter that produces gate 3's rows is not unit-tested here; it
# runs on the Bichon Host and is exercised end to end by a `--no-input` run.
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

report 'bichon-expunge'
