#!/usr/bin/env bash
#
# tests/bichon-archive.test.sh
#
# Unit tests for what the Email Archive accepts as a message and how it keys it:
# the check that a downloaded payload is a body at all, the extractor that keys a
# sidecar on that body's Message-ID, and the passes that repair entries written
# before either existed (ADR-0013, ADR-0015).
#
# No Bichon, no live archive — every case is a body written into a temp
# directory, and the one function that would reach the API is stubbed. Both
# checks are worth pinning because both were first written in the obvious form
# and the obvious form was wrong: a Message-ID reader that did not unfold RFC
# 5322 continuation lines reported 106 duplicate bodies where the corpus held
# none, and a payload check anchored on the first line would reject the three
# live bodies that open with an mbox From_ line.
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

# The function must fail on its own rather than rely on a caller checking
# readability first. Falling through to the hash branch when awk cannot read the
# file yields a bare "sha256:", which every unreadable body would then share.
printf 'Message-ID: <unreadable@example.com>\n\nbody\n' >"${WORK}/noperm.eml"
chmod 000 "${WORK}/noperm.eml"
assert_fails 'an unreadable body returns non-zero, not a bare prefix' \
  canonical_message_id "${WORK}/noperm.eml"
assert_eq 'an unreadable body prints no id' '' \
  "$(canonical_message_id "${WORK}/noperm.eml" 2>/dev/null || true)"
chmod 644 "${WORK}/noperm.eml"

assert_fails 'a missing body returns non-zero' \
  canonical_message_id "${WORK}/does-not-exist.eml"

printf '\n== has_header_block\n'

# Write $2 as a body and answer whether the archive would accept it.
body_of() {
  printf '%s' "$2" >"${WORK}/$1.eml"
  printf '%s' "${WORK}/$1.eml"
}

assert_succeeds 'headers followed by a blank line are a message' \
  has_header_block "$(body_of hb_plain 'From: a@b
Subject: s

body
')"

assert_succeeds 'a CRLF separator is a separator' \
  has_header_block "$(
    printf 'From: a@b\r\nSubject: s\r\n\r\nbody\r\n' >"${WORK}/hb_crlf.eml"
    printf '%s' "${WORK}/hb_crlf.eml"
  )"

# Three bodies in the live corpus open with an mbox From_ line. A first-line
# "must look like a header field" rule would reject all three on every run,
# re-downloading them forever — this is the case that ruled that rule out.
assert_succeeds 'an mbox From_ line before the headers is still a message' \
  has_header_block "$(body_of hb_mbox 'From someone@example.com Mon Jul 27 09:00:00 2026
From: a@b

body
')"

# The defect: Bichon answers 200 with nothing when its blob store holds no
# content for an envelope it still indexes, and curl --fail reports success.
: >"${WORK}/hb_empty.eml"
assert_fails 'an empty payload is not a message' \
  has_header_block "${WORK}/hb_empty.eml"

assert_fails 'headers cut off before the separator are not a message' \
  has_header_block "$(body_of hb_cut 'From: a@b
Subject: truncated mid')"

assert_fails 'a separator with no headers before it is not a message' \
  has_header_block "$(body_of hb_nofields '
body only
')"

# The skip guard asks this of every envelope it sees, including ones never
# archived, so a missing path is an ordinary answer rather than an error.
assert_fails 'a path that does not exist is not a message' \
  has_header_block "${WORK}/hb_absent.eml"

printf 'From: a@b\n\nbody\n' >"${WORK}/hb_locked.eml"
chmod 000 "${WORK}/hb_locked.eml"
assert_fails 'an unreadable body is not credited as a message' \
  has_header_block "${WORK}/hb_locked.eml"
chmod 644 "${WORK}/hb_locked.eml"

printf '\n== download_message\n'

# The suite's only stub. curl_auth is the seam between the archive and Bichon,
# and what is under test is what the archive does with a payload — not how the
# payload arrived. STUB_PAYLOAD is written wherever --output points, and
# STUB_STATUS is curl's verdict, which for this defect is always success.
STUB_PAYLOAD=''
STUB_STATUS=0
curl_auth() {
  local out=''
  while [ $# -gt 0 ]; do
    case "$1" in
      --output)
        out="$2"
        shift 2
        ;;
      *) shift ;;
    esac
  done
  [ -n "${out}" ] && printf '%s' "${STUB_PAYLOAD}" >"${out}"
  return "${STUB_STATUS}"
}

DL_DIR="${WORK}/download"
mkdir -p "${DL_DIR}"

STUB_PAYLOAD=''
assert_fails 'an empty 200 is refused' download_message 1 900 "${DL_DIR}/900.eml"
assert_eq 'the refusal leaves neither a body nor a temp file' '' \
  "$(find "${DL_DIR}" -name '900.eml*' -printf '%f\n')"

STUB_PAYLOAD='From: a@b
Subject: cut off here'
assert_fails 'a 200 truncated inside the headers is refused' \
  download_message 1 901 "${DL_DIR}/901.eml"
assert_eq 'a refused truncation leaves nothing behind' '' \
  "$(find "${DL_DIR}" -name '901.eml*' -printf '%f\n')"

STUB_PAYLOAD='Message-ID: <downloaded@example.com>

body
'
assert_succeeds 'a payload with a header block is published' \
  download_message 1 902 "${DL_DIR}/902.eml"
assert_eq 'the published body is group-readable' '640' \
  "$(stat -c '%a' "${DL_DIR}/902.eml")"

STUB_STATUS=1
assert_fails 'a curl failure is still a failure' \
  download_message 1 903 "${DL_DIR}/903.eml"
STUB_STATUS=0

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
rm "${BACKFILL_DIR}/14.eml" "${BACKFILL_DIR}/14.meta.json"

# An unreadable body must be one counted failure, not an abort. The pass runs
# per account inside the hourly archive, so aborting would stop new mail being
# archived for every account after this one.
printf 'Message-ID: <locked@example.com>\n\nbody\n' >"${BACKFILL_DIR}/15.eml"
printf '{"folder":"INBOX","tags":[]}\n' >"${BACKFILL_DIR}/15.meta.json"
printf 'Message-ID: <after@example.com>\n\nbody\n' >"${BACKFILL_DIR}/16.eml"
printf '{"folder":"INBOX","tags":[]}\n' >"${BACKFILL_DIR}/16.meta.json"
chmod 000 "${BACKFILL_DIR}/15.eml"

assert_fails 'an unreadable body fails the run' backfill_message_ids 'c@d.com'
assert_eq 'a later sidecar is still repaired after an unreadable body' \
  '{"folder":"INBOX","message_id":"after@example.com"}' \
  "$(jq -cS . "${BACKFILL_DIR}/16.meta.json")"

chmod 644 "${BACKFILL_DIR}/15.eml"
rm "${BACKFILL_DIR}/15.eml" "${BACKFILL_DIR}/15.meta.json" \
  "${BACKFILL_DIR}/16.eml" "${BACKFILL_DIR}/16.meta.json"

printf '\n== write_meta_sidecar refuses to publish a sidecar it could not build\n'

printf 'Message-ID: <unpublished@example.com>\n\nbody\n' >"${NEW_DIR}/2.eml"
assert_fails 'a malformed envelope fails rather than publishing' \
  write_meta_sidecar "${NEW_DIR}/2.meta.json" "${NEW_DIR}/2.eml" 'not json'
assert_eq 'no sidecar is left behind by the failed write' '' \
  "$(find "${NEW_DIR}" -name '2.meta.json*' -printf '%f\n')"

report 'bichon-archive'
