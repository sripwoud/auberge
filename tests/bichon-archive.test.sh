#!/usr/bin/env bash
#
# tests/bichon-archive.test.sh
#
# Unit tests for the Email Archive's message identity: the extractor that keys a
# sidecar on the body's Message-ID, and the pass that repairs sidecars written
# before that key existed (ADR-0013).
#
# No Bichon, no API, no live archive — every case is a body written into a temp
# directory. The extractor is the piece worth pinning: the defect that motivated
# ADR-0013 was first mis-measured with a Message-ID reader that did not unfold
# RFC 5322 continuation lines, which reported 106 duplicate bodies where the
# corpus held none.
#
# Run: ./tests/bichon-archive.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly TEMPLATE="${REPO_ROOT}/ansible/roles/bichon/templates/bichon-archive.sh.j2"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

# The template's only Jinja substitutions are two integer defaults inside
# ${VAR:=…}, so rendering for a test is a numeric replacement. The env vars below
# satisfy the script's ${VAR:?required} guards, which run at source time.
sed 's/{{[^}]*}}/1/g' "${TEMPLATE}" >"${WORK}/bichon-archive.sh"
export BICHON_BASE_URL='http://127.0.0.1:1'
export BICHON_API_TOKEN='unused'
export BICHON_ARCHIVE_DIR="${WORK}/archive"
export BICHON_ARCHIVE_STATE_DIR="${WORK}/state"
# shellcheck source=/dev/null
source "${WORK}/bichon-archive.sh"

# Write $2 as a body and print the id the archive would key it on.
id_of() {
  local name="$1" body="$2"
  printf '%s' "${body}" >"${WORK}/${name}.eml"
  canonical_message_id "${WORK}/${name}.eml"
}

sha256_of() {
  printf 'sha256:%s' "$(sha256sum "$1" | cut -d' ' -f1)"
}

printf '== canonical_message_id\n'

assert_eq 'plain header' 'plain@example.com' \
  "$(id_of plain 'From: a@b
Message-ID: <plain@example.com>
Subject: s

body
')"

assert_eq 'header already bare of angle brackets' 'bare@example.com' \
  "$(id_of bare 'From: a@b
Message-ID: bare@example.com

body
')"

# The regression that produced the phantom duplicate count. Exchange emits this
# shape routinely; a line-anchored match reads it as empty.
assert_eq 'folded header unfolds to the same id as an unfolded one' 'folded@example.com' \
  "$(id_of folded 'From: a@b
Message-ID:
 <folded@example.com>
Subject: s

body
')"

assert_eq 'folded header across two continuation lines' 'split@example.com' \
  "$(id_of split 'From: a@b
Message-ID:
 <split@
	example.com>

body
')"

assert_eq 'header name is matched case-insensitively' 'lower@example.com' \
  "$(id_of lower 'From: a@b
message-id: <lower@example.com>

body
')"

assert_eq 'CRLF line endings leave no carriage return in the id' 'crlf@example.com' \
  "$(
    printf 'From: a@b\r\nMessage-ID: <crlf@example.com>\r\n\r\nbody\r\n' >"${WORK}/crlf.eml"
    canonical_message_id "${WORK}/crlf.eml"
  )"

assert_eq 'a repeated header keeps the first value' 'first@example.com' \
  "$(id_of dup 'From: a@b
Message-ID: <first@example.com>
Message-ID: <second@example.com>

body
')"

# Scanning stops at the header/body separator, so a quoted Message-ID in a reply
# cannot be mistaken for this message's own.
assert_eq 'a Message-ID in the body is not read as the header' \
  "$(sha256_of "$(printf 'From: a@b
Subject: s

Message-ID: <quoted@example.com>
' >"${WORK}/inbody.eml" && echo "${WORK}/inbody.eml")")" \
  "$(canonical_message_id "${WORK}/inbody.eml")"

printf 'From: a@b
Subject: Email Quota Usage: 80%%

used 80%%
' >"${WORK}/quota80.eml"
printf 'From: a@b
Subject: Email Quota Usage: 100%%

used 100%%
' >"${WORK}/quota100.eml"

assert_eq 'a body with no Message-ID is keyed by its sha256' \
  "$(sha256_of "${WORK}/quota80.eml")" \
  "$(canonical_message_id "${WORK}/quota80.eml")"

# The live archive holds two such notifications. Under an empty key they collide
# and one message's coverage vanishes; under the hash they stay distinct.
assert_fails 'two different header-less bodies get different ids' \
  test "$(canonical_message_id "${WORK}/quota80.eml")" = \
  "$(canonical_message_id "${WORK}/quota100.eml")"

: >"${WORK}/empty.eml"
assert_eq 'an empty body still yields an id' \
  "$(sha256_of "${WORK}/empty.eml")" \
  "$(canonical_message_id "${WORK}/empty.eml")"

printf '\n== write_meta_sidecar\n'

mkdir -p "${BICHON_ARCHIVE_DIR}/a@b.com/2026/07"
NEW_DIR="${BICHON_ARCHIVE_DIR}/a@b.com/2026/07"
printf 'Message-ID: <written@example.com>\n\nbody\n' >"${NEW_DIR}/1.eml"
write_meta_sidecar "${NEW_DIR}/1.meta.json" "${NEW_DIR}/1.eml" \
  '{"id":1,"mailbox_name":"INBOX","tags":[],"subject":"s"}'

assert_eq 'sidecar records folder and message_id only' \
  '{"folder":"INBOX","message_id":"written@example.com"}' \
  "$(jq -cS . "${NEW_DIR}/1.meta.json")"

assert_eq 'sidecar is group-readable' '640' \
  "$(stat -c '%a' "${NEW_DIR}/1.meta.json")"

printf '\n== backfill_message_ids\n'

BACKFILL_DIR="${BICHON_ARCHIVE_DIR}/c@d.com/2026/07"
mkdir -p "${BACKFILL_DIR}"

printf 'Message-ID:\n <legacy@example.com>\n\nbody\n' >"${BACKFILL_DIR}/10.eml"
printf '{"folder":"INBOX","tags":[]}\n' >"${BACKFILL_DIR}/10.meta.json"
printf 'Subject: no header\n\nbody\n' >"${BACKFILL_DIR}/11.eml"
printf '{"folder":"Archive","tags":[]}\n' >"${BACKFILL_DIR}/11.meta.json"
printf 'Message-ID: <ignored@example.com>\n\nbody\n' >"${BACKFILL_DIR}/12.eml"
printf '{"folder":"Sent","message_id":"kept@example.com"}\n' >"${BACKFILL_DIR}/12.meta.json"

KEPT_BEFORE="$(cat "${BACKFILL_DIR}/12.meta.json")"

assert_succeeds 'a repairable account backfills cleanly' \
  backfill_message_ids 'c@d.com'

assert_eq 'a folded legacy sidecar gains its message_id and loses tags' \
  '{"folder":"INBOX","message_id":"legacy@example.com"}' \
  "$(jq -cS . "${BACKFILL_DIR}/10.meta.json")"

assert_eq 'a header-less legacy sidecar is keyed by the body hash' \
  "{\"folder\":\"Archive\",\"message_id\":\"$(sha256_of "${BACKFILL_DIR}/11.eml")\"}" \
  "$(jq -cS . "${BACKFILL_DIR}/11.meta.json")"

# Write-once still holds for entries that already carry the key: the pass repairs
# what is missing, it does not re-derive what is recorded.
assert_eq 'an already-keyed sidecar is left byte-for-byte alone' \
  "${KEPT_BEFORE}" "$(cat "${BACKFILL_DIR}/12.meta.json")"

assert_eq 'a repaired sidecar is group-readable' '640' \
  "$(stat -c '%a' "${BACKFILL_DIR}/10.meta.json")"

MTIME_BEFORE="$(stat -c '%Y' "${BACKFILL_DIR}/10.meta.json")"
assert_succeeds 'a second pass converges' backfill_message_ids 'c@d.com'
assert_eq 'a converged pass rewrites nothing' \
  "${MTIME_BEFORE}" "$(stat -c '%Y' "${BACKFILL_DIR}/10.meta.json")"

assert_succeeds 'an account with no archive directory is a no-op' \
  backfill_message_ids 'absent@e.com'

printf '{"folder":"INBOX","tags":[]}\n' >"${BACKFILL_DIR}/13.meta.json"
assert_fails 'a sidecar with no body fails the run' \
  backfill_message_ids 'c@d.com'
rm "${BACKFILL_DIR}/13.meta.json"

printf 'Message-ID: <orphan@example.com>\n\nbody\n' >"${BACKFILL_DIR}/14.eml"
printf '{"tags":[]}\n' >"${BACKFILL_DIR}/14.meta.json"
assert_fails 'a sidecar with no folder to preserve fails the run' \
  backfill_message_ids 'c@d.com'
assert_eq 'an unrepairable sidecar is left as it was' '{"tags":[]}' \
  "$(jq -cS . "${BACKFILL_DIR}/14.meta.json")"

report 'bichon-archive'
