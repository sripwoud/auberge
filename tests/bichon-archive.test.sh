#!/usr/bin/env bash
#
# tests/bichon-archive.test.sh
#
# Unit tests for what the Email Archive accepts as a message, how it keys it, and
# which copy of a message it declines to write: the check that a downloaded
# payload is a body at all, the extractor that keys a sidecar on that body's
# Message-ID, the passes that repair entries written before either existed
# (ADR-0013, ADR-0015), and the skip guard that decides whether the archive
# already holds an offered message (#455).
#
# No Bichon, no live archive — every case is a body written into a temp
# directory, and the three functions that would reach the API are stubbed. Each
# check is worth pinning because each was first written in the obvious form and
# the obvious form was wrong: a Message-ID reader that did not unfold RFC 5322
# continuation lines reported 106 duplicate bodies where the corpus held none, a
# payload check anchored on the first line would reject the three live bodies
# that open with an mbox From_ line, and a skip guard that asked whether
# `<envelope-id>.eml` existed wrote 825 duplicates in one tick the moment Bichon
# re-minted its envelope ids.
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
write_meta_sidecar "${NEW_DIR}/1.meta.json" \
  "$(canonical_message_id "${NEW_DIR}/1.eml")" \
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

printf '\n== repair_broken_bodies\n'

REPAIR_DIR="${BICHON_ARCHIVE_DIR}/e@f.com/2026/03"
mkdir -p "${REPAIR_DIR}"

# What the corpus holds today: a 0-byte body published by curl --fail, and a
# sidecar keyed on the hash of nothing because that is what the extractor read.
: >"${REPAIR_DIR}/20.eml"
printf '{"folder":"Sent","message_id":"%s"}\n' "$(sha256_of "${REPAIR_DIR}/20.eml")" \
  >"${REPAIR_DIR}/20.meta.json"

# Left alone by the sweep: a body that is already a message, and one that opens
# with an mbox From_ line.
printf 'Message-ID: <intact@example.com>\n\nbody\n' >"${REPAIR_DIR}/21.eml"
printf '{"folder":"INBOX","message_id":"intact@example.com"}\n' >"${REPAIR_DIR}/21.meta.json"
printf 'From someone@example.com Mon Jul 27 09:00:00 2026\nMessage-ID: <mbox@example.com>\n\nbody\n' \
  >"${REPAIR_DIR}/22.eml"
printf '{"folder":"INBOX","message_id":"mbox@example.com"}\n' >"${REPAIR_DIR}/22.meta.json"

INTACT_MTIME="$(stat -c '%Y' "${REPAIR_DIR}/21.eml")"
MBOX_MTIME="$(stat -c '%Y' "${REPAIR_DIR}/22.eml")"

STUB_PAYLOAD='Message-ID: <refetched@example.com>

the body that was there all along
'
assert_succeeds 'an account whose broken body can be refetched repairs cleanly' \
  repair_broken_bodies 7 'e@f.com'

assert_eq 'the empty body is replaced by the message' \
  'Message-ID: <refetched@example.com>' \
  "$(head -1 "${REPAIR_DIR}/20.eml")"

# The stale key is sha256: of nothing, which every empty body shares. Re-keying
# is the point of the repair, not a side effect of it.
assert_eq 'the sidecar is re-keyed off the refetched body, keeping its folder' \
  '{"folder":"Sent","message_id":"refetched@example.com"}' \
  "$(jq -cS . "${REPAIR_DIR}/20.meta.json")"

assert_eq 'the repaired body is group-readable' '640' \
  "$(stat -c '%a' "${REPAIR_DIR}/20.eml")"

assert_eq 'an intact body is not refetched' \
  "${INTACT_MTIME}" "$(stat -c '%Y' "${REPAIR_DIR}/21.eml")"
assert_eq 'an mbox From_ body is not refetched' \
  "${MBOX_MTIME}" "$(stat -c '%Y' "${REPAIR_DIR}/22.eml")"

assert_succeeds 'a second pass finds nothing to do' \
  repair_broken_bodies 7 'e@f.com'
assert_eq 'the repaired body is not rewritten by the second pass' \
  'Message-ID: <refetched@example.com>' \
  "$(head -1 "${REPAIR_DIR}/20.eml")"

assert_succeeds 'an account with no archive directory is a no-op' \
  repair_broken_bodies 7 'absent@e.com'

# Bichon no longer holds the envelope, or answers 200 with nothing again. The
# archive does not have that message and cannot fetch it, so the run fails every
# tick until an operator removes the entry.
: >"${REPAIR_DIR}/23.eml"
printf '{"folder":"INBOX","message_id":"stale"}\n' >"${REPAIR_DIR}/23.meta.json"
STUB_STATUS=1
assert_fails 'a body that cannot be refetched fails the run' \
  repair_broken_bodies 7 'e@f.com'
assert_eq 'a body that cannot be refetched is left as it was' '0' \
  "$(stat -c '%s' "${REPAIR_DIR}/23.eml")"
STUB_STATUS=0

STUB_PAYLOAD='Message-ID: <orphan@example.com>

body
'
rm "${REPAIR_DIR}/23.meta.json"
assert_fails 'a body with no sidecar fails the run' \
  repair_broken_bodies 7 'e@f.com'
assert_eq 'the body is still refetched before the sidecar is missed' \
  'Message-ID: <orphan@example.com>' \
  "$(head -1 "${REPAIR_DIR}/23.eml")"
rm "${REPAIR_DIR}/23.eml"

: >"${REPAIR_DIR}/24.eml"
printf '{"message_id":"stale"}\n' >"${REPAIR_DIR}/24.meta.json"
assert_fails 'a sidecar with no folder to preserve fails the run' \
  repair_broken_bodies 7 'e@f.com'
assert_eq 'that sidecar is left as it was' '{"message_id":"stale"}' \
  "$(jq -cS . "${REPAIR_DIR}/24.meta.json")"
rm "${REPAIR_DIR}/24.eml" "${REPAIR_DIR}/24.meta.json"

printf '\n== write_meta_sidecar refuses to publish a sidecar it could not build\n'

assert_fails 'a malformed envelope fails rather than publishing' \
  write_meta_sidecar "${NEW_DIR}/2.meta.json" 'unpublished@example.com' 'not json'
assert_eq 'no sidecar is left behind by the failed write' '' \
  "$(find "${NEW_DIR}" -name '2.meta.json*' -printf '%f\n')"

printf '\n== load_archived_message_ids\n'

SET_ACCOUNT='set@example.com'
SET_DIR="${BICHON_ARCHIVE_DIR}/${SET_ACCOUNT}"
mkdir -p "${SET_DIR}/2026/07" "${SET_DIR}/2026/08"
printf '{"folder":"INBOX","message_id":"july@example.com"}\n' >"${SET_DIR}/2026/07/30.meta.json"
printf '{"folder":"Sent","message_id":"august@example.com"}\n' >"${SET_DIR}/2026/08/31.meta.json"
# RFC 5322 §3.6.4 permits a domain-literal on the right of a msg-id, so a key can
# carry the `]` that ends an array subscript. Unquoted, the subscript would not
# parse and the id would be absent from the set it was just added to.
printf '{"folder":"INBOX","message_id":"lit@[10.0.0.1]"}\n' >"${SET_DIR}/2026/08/32.meta.json"

assert_succeeds 'a keyed account loads' load_archived_message_ids "${SET_ACCOUNT}"
assert_eq 'every sidecar of every month partition contributes one key' '3' \
  "${#ARCHIVED_MESSAGE_IDS[@]}"
assert_eq 'a key from one partition is present' 'archived' \
  "${ARCHIVED_MESSAGE_IDS['july@example.com']+archived}"
assert_eq 'a key from another partition is present' 'archived' \
  "${ARCHIVED_MESSAGE_IDS['august@example.com']+archived}"
assert_eq 'a domain-literal id survives as a key' 'archived' \
  "${ARCHIVED_MESSAGE_IDS['lit@[10.0.0.1]']+archived}"
assert_eq 'an id not in the corpus is absent' '' \
  "${ARCHIVED_MESSAGE_IDS['absent@example.com']+archived}"

# The set is rebuilt per account, so the previous account's keys must not leak
# into the next one's membership test — they would suppress downloads of mail
# that account does not hold.
assert_succeeds 'an account with no archive directory is a no-op' \
  load_archived_message_ids 'absent@e.com'
assert_eq "the no-op leaves an empty set, not the last account's" '0' \
  "${#ARCHIVED_MESSAGE_IDS[@]}"

# backfill_message_ids runs before this and fails the run over an unkeyed
# sidecar, so contributing nothing is right; contributing an empty key would make
# every unkeyed message look archived.
printf '{"folder":"INBOX","tags":[]}\n' >"${SET_DIR}/2026/08/33.meta.json"
assert_succeeds 'an unkeyed sidecar does not fail the load' \
  load_archived_message_ids "${SET_ACCOUNT}"
assert_eq 'an unkeyed sidecar contributes no key at all' '3' \
  "${#ARCHIVED_MESSAGE_IDS[@]}"
rm "${SET_DIR}/2026/08/33.meta.json"

# A set quietly short of one entry is a duplicate written on the next tick, so an
# unreadable sidecar fails rather than under-reporting. The literal "message_id"
# keeps grep -L from listing it as unkeyed, which is what leaves it for jq.
printf '{"folder":"INBOX","message_id":\n' >"${SET_DIR}/2026/08/34.meta.json"
assert_fails 'an unparseable sidecar fails the load' \
  load_archived_message_ids "${SET_ACCOUNT}"
rm "${SET_DIR}/2026/08/34.meta.json"

printf '\n== process_account skips a message the archive already holds\n'

# The two remaining seams to Bichon. search_messages answers with one page of
# STUB_ENVELOPES; write_tag_snapshot is an independent API walk with nothing to
# say about the skip guard. Downloads still run through the curl_auth stub above,
# so STUB_PAYLOAD is the body Bichon serves.
STUB_ENVELOPES='[]'
search_messages() {
  jq -nc --argjson items "${STUB_ENVELOPES}" '{total_pages: 1, items: $items}'
}
write_tag_snapshot() { return 0; }

mkdir -p "${BICHON_ARCHIVE_STATE_DIR}"

# 2026-08-10T00:00:00Z — the partition the archive derives from the envelope date.
AUG_MS=1786320000000

# Report the counters process_account logs, which is where "already archived" is
# observable: `skipped` is a filename match, `deduped` an identity match.
counters_of() {
  local account="$1" out
  out=$(process_account 7 "${account}" 2>&1)
  printf '%s' "${out}" | grep -o 'processed=.*failures=[0-9]*' | tail -1
}

DEDUP_ACCOUNT='dedup@example.com'
DEDUP_DIR="${BICHON_ARCHIVE_DIR}/${DEDUP_ACCOUNT}/2026/08"
mkdir -p "${DEDUP_DIR}"

# The corpus as the v2 upgrade left it: a body archived under the numeric
# envelope id Bichon minted before the migration, keyed on its Message-ID.
printf 'Message-ID: <already@example.com>\n\nthe body already archived\n' \
  >"${DEDUP_DIR}/8437886698289967.eml"
printf '{"folder":"INBOX","message_id":"already@example.com"}\n' \
  >"${DEDUP_DIR}/8437886698289967.meta.json"

# The same message, offered under the UUID the migration re-minted for it. This
# is #455 exactly: no filename can connect the two, and the run that could not
# wrote 825 duplicate files in one tick.
STUB_ENVELOPES="[{\"id\":\"0a74339f-604c-4bc6-be67-a9286e6449e0\",\"date\":${AUG_MS},\"mailbox_name\":\"INBOX\"}]"
STUB_PAYLOAD='Message-ID: <already@example.com>

the body already archived
'
assert_eq 'a message re-offered under a fresh envelope id is deduped, not written' \
  'processed=0 skipped=0 deduped=1 failures=0' \
  "$(counters_of "${DEDUP_ACCOUNT}")"
assert_eq 'the corpus still holds one body for that message' '1' \
  "$(find "${DEDUP_DIR}" -name '*.eml' | wc -l)"
assert_eq 'no sidecar is written for the copy that was not published' '1' \
  "$(find "${DEDUP_DIR}" -name '*.meta.json' | wc -l)"
assert_eq 'the discarded download leaves no staging file behind' '' \
  "$(find "${BICHON_ARCHIVE_DIR}/${DEDUP_ACCOUNT}" -name '*.incoming*' -printf '%f\n')"
assert_eq 'the body already archived is not rewritten' \
  'the body already archived' \
  "$(tail -1 "${DEDUP_DIR}/8437886698289967.eml")"

# Genuinely new mail in the same window must still land — the guard has to
# distinguish "already archived" from "not seen", which is the distinction a
# filename lost.
STUB_ENVELOPES="[{\"id\":\"11111111-2222-3333-4444-555555555555\",\"date\":${AUG_MS},\"mailbox_name\":\"INBOX\"}]"
STUB_PAYLOAD='Message-ID: <fresh@example.com>

new mail
'
assert_eq 'a message the archive does not hold is downloaded and published' \
  'processed=1 skipped=0 deduped=0 failures=0' \
  "$(counters_of "${DEDUP_ACCOUNT}")"
assert_eq 'it is published under the envelope id in force now' \
  'Message-ID: <fresh@example.com>' \
  "$(head -1 "${DEDUP_DIR}/11111111-2222-3333-4444-555555555555.eml")"
assert_eq 'its sidecar is keyed on the body it just published' \
  '{"folder":"INBOX","message_id":"fresh@example.com"}' \
  "$(jq -cS . "${DEDUP_DIR}/11111111-2222-3333-4444-555555555555.meta.json")"

# The next tick re-lists the same 24h window. The message published above now
# answers to its own filename, so it costs no download at all — which is what
# bounds the identity test to the mail whose id regime has actually moved.
assert_eq 'the message published last run is skipped by filename, not refetched' \
  'processed=0 skipped=1 deduped=0 failures=0' \
  "$(counters_of "${DEDUP_ACCOUNT}")"

# One page holding both, to pin that the counters are per envelope and that
# neither decision consumes the other.
STUB_ENVELOPES="[{\"id\":\"0a74339f-604c-4bc6-be67-a9286e6449e0\",\"date\":${AUG_MS},\"mailbox_name\":\"INBOX\"},{\"id\":\"11111111-2222-3333-4444-555555555555\",\"date\":${AUG_MS},\"mailbox_name\":\"INBOX\"}]"
STUB_PAYLOAD='Message-ID: <already@example.com>

the body already archived
'
assert_eq 'a mixed page counts one filename skip and one identity skip' \
  'processed=0 skipped=1 deduped=1 failures=0' \
  "$(counters_of "${DEDUP_ACCOUNT}")"
assert_eq 'the mixed page adds no file' '2' \
  "$(find "${DEDUP_DIR}" -name '*.eml' | wc -l)"

# The 51 byte-identical 187-byte notifications in the live corpus carry no
# Message-ID and no Date, so they share one `sha256:` key and accumulated one
# copy per full pass. Under the hash they are one message by the archive's own
# definition of identity (ADR-0013), so the second copy is a duplicate.
HASHED_ACCOUNT='hashed@example.com'
HASHED_DIR="${BICHON_ARCHIVE_DIR}/${HASHED_ACCOUNT}/2026/08"
mkdir -p "${HASHED_DIR}"
printf 'From: a@b\nSubject: Email Quota Usage: 80%%\n\nused 80%%\n' >"${HASHED_DIR}/700.eml"
printf '{"folder":"INBOX","message_id":"%s"}\n' "$(sha256_of "${HASHED_DIR}/700.eml")" \
  >"${HASHED_DIR}/700.meta.json"

STUB_ENVELOPES="[{\"id\":\"99999999-8888-7777-6666-555555555555\",\"date\":${AUG_MS},\"mailbox_name\":\"INBOX\"}]"
STUB_PAYLOAD='From: a@b
Subject: Email Quota Usage: 80%

used 80%
'
assert_eq 'a header-less body already archived is deduped by its hash' \
  'processed=0 skipped=0 deduped=1 failures=0' \
  "$(counters_of "${HASHED_ACCOUNT}")"
assert_eq 'no second copy of it is written' '1' \
  "$(find "${HASHED_DIR}" -name '*.eml' | wc -l)"

# A body already published whose sidecar write failed on an earlier run. Not a
# dedup candidate even when its identity is already held: it is in the corpus
# either way, and an unkeyed body is one the coverage gate cannot count.
printf 'Message-ID: <already@example.com>\n\nthe body already archived\n' \
  >"${DEDUP_DIR}/22222222-3333-4444-5555-666666666666.eml"
STUB_ENVELOPES="[{\"id\":\"22222222-3333-4444-5555-666666666666\",\"date\":${AUG_MS},\"mailbox_name\":\"Archive\"}]"
assert_eq 'an unkeyed body already in the corpus is keyed, not deduped' \
  'processed=1 skipped=0 deduped=0 failures=0' \
  "$(counters_of "${DEDUP_ACCOUNT}")"
assert_eq 'it is keyed from the body on disk, with the folder it was seen in' \
  '{"folder":"Archive","message_id":"already@example.com"}' \
  "$(jq -cS . "${DEDUP_DIR}/22222222-3333-4444-5555-666666666666.meta.json")"
rm "${DEDUP_DIR}/22222222-3333-4444-5555-666666666666.eml" \
  "${DEDUP_DIR}/22222222-3333-4444-5555-666666666666.meta.json"

# Downloading on the basis of a set that could not be built is what wrote the
# duplicates, so an unreadable corpus takes the account's pass with it instead of
# proceeding blind.
printf '{"folder":"INBOX","message_id":\n' >"${DEDUP_DIR}/broken.meta.json"
STUB_ENVELOPES="[{\"id\":\"33333333-4444-5555-6666-777777777777\",\"date\":${AUG_MS},\"mailbox_name\":\"INBOX\"}]"
STUB_PAYLOAD='Message-ID: <never@example.com>

must not be downloaded
'
BLIND_LOG=$(process_account 7 "${DEDUP_ACCOUNT}" 2>&1)
assert_eq 'an unreadable corpus stops the account before any download' \
  'identity set failed' \
  "$(printf '%s' "${BLIND_LOG}" | grep -o 'identity set failed' | head -1)"
assert_eq 'nothing was downloaded on the strength of a set that failed to build' '' \
  "$(find "${DEDUP_DIR}" -name '33333333-*' -printf '%f\n')"
rm "${DEDUP_DIR}/broken.meta.json"

report 'bichon-archive'
