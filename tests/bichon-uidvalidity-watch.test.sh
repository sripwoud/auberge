#!/usr/bin/env bash
#
# tests/bichon-uidvalidity-watch.test.sh
#
# Unit tests for the Rebuild Latch: the journal watcher that reports a Bichon
# mailbox cache rebuild and keeps reporting it until an operator acknowledges
# (ADR-0014).
#
# No journal, no systemd, no Bichon — journalctl is a stub on PATH whose output
# and exit code each case stages. The properties worth pinning are the ones a
# live test could not reproduce on demand: that the latch survives a run which
# reads nothing new (systemd would otherwise clear the failed state on the next
# tick), and that an unreadable journal is not reported as "no rebuild".
#
# Run: ./tests/bichon-uidvalidity-watch.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly SCRIPT="${REPO_ROOT}/ansible/roles/bichon/files/bichon-uidvalidity-watch.sh"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

readonly BIN="${WORK}/bin"
readonly STUB_OUT="${WORK}/journal.out"
readonly STUB_RC="${WORK}/journal.rc"
readonly STUB_ARGS="${WORK}/journal.args"

mkdir -p "${BIN}" "${WORK}/state"
cat >"${BIN}/journalctl" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${STUB_ARGS}"
cat "${STUB_OUT}"
exit "\$(cat "${STUB_RC}")"
STUB
chmod 0755 "${BIN}/journalctl"
export PATH="${BIN}:${PATH}"

# One journal line as journalctl --output=short-iso-precise renders it. Bichon
# formats the uid values with {:#?}, so the entry continues onto the lines staged
# after it in the multiline case below; the mailbox name and the timestamp are
# both on this one.
readonly SIGNAL_LINE="2026-07-30T09:14:02.117431+0000 vps bichon[811]: Account 1: Mailbox 'INBOX' detected with changed uid_validity (local: Some("

stage_journal() {
  local rc="$1" line
  shift
  : >"${STUB_OUT}"
  for line in "$@"; do printf '%s\n' "${line}" >>"${STUB_OUT}"; done
  printf '%s' "${rc}" >"${STUB_RC}"
}

# Runs the watcher against ${WORK}/state/<name>, printing its exit status.
# Re-running the same name reuses that state dir, which is how the latch and
# cursor are carried between ticks.
run_case() {
  local name="$1" rc="$2"
  shift 2
  stage_journal "${rc}" "$@"
  local status=0
  BICHON_UIDVALIDITY_STATE_DIR="${WORK}/state/${name}" \
    bash "${SCRIPT}" >/dev/null 2>>"${WORK}/${name}.stderr" || status=$?
  printf '%s' "${status}"
}

# As run_case, against a journal held quiet, for a unit other than the default.
watch_unit() {
  local name="$1" unit="$2"
  stage_journal 0
  local status=0
  BICHON_UIDVALIDITY_STATE_DIR="${WORK}/state/${name}" \
    BICHON_UIDVALIDITY_UNIT="${unit}" \
    bash "${SCRIPT}" >/dev/null 2>>"${WORK}/${name}.stderr" || status=$?
  printf '%s' "${status}"
}

latch_of() {
  local latch="${WORK}/state/${1}/rebuilds.log"
  [[ -f "${latch}" ]] || return 0
  cat "${latch}"
}

printf '== a clean journal\n'

assert_eq 'unremarkable entries exit 0' '0' \
  "$(run_case quiet 0 'Account 1: sync finished, 12 new envelopes')"

assert_eq 'and write no latch' '' "$(latch_of quiet)"

assert_eq 'an empty journal exits 0' '0' "$(run_case empty 0)"

printf '\n== a detected rebuild\n'

assert_eq 'the signal exits 1' '1' \
  "$(run_case detect 0 'Account 1: sync started' "${SIGNAL_LINE}")"

assert_eq 'the matching line is latched verbatim' "${SIGNAL_LINE}" "$(latch_of detect)"

assert_eq 'the continuation lines of the entry are not latched' '1' \
  "$(
    run_case multiline 0 "${SIGNAL_LINE}" '    1758268301,' '), remote: Some(' '    1758268999,' ')' \
      >/dev/null
    latch_of multiline | wc -l
  )"

assert_eq 'two rebuilds in one run are both latched' '2' \
  "$(
    run_case pair 0 "${SIGNAL_LINE}" 'Account 1: unrelated' "${SIGNAL_LINE}" >/dev/null
    latch_of pair | wc -l
  )"

printf '\n== the latch\n'

# The property the whole design turns on. systemd clears a unit's failed state on
# the next successful start, so a run that reads nothing new must still fail, or
# the hourly tick after a rebuild would erase the alert.
assert_eq 'a rebuild latches' '1' "$(run_case persist 0 "${SIGNAL_LINE}")"
assert_eq 'a later run reading nothing new still fails' '1' "$(run_case persist 0)"
assert_eq 'without duplicating the latched line' '1' "$(latch_of persist | wc -l)"

rm "${WORK}/state/persist/rebuilds.log"
assert_eq 'deleting the latch acknowledges the alert' '0' "$(run_case persist 0)"

printf '\n== operational errors\n'

# An unreadable journal must not be reported as "no rebuild": the run could not
# answer the question, which is exit 2 everywhere else in auberge.
assert_eq 'an unreadable journal exits 2' '2' "$(run_case unreadable 1)"
assert_eq 'and writes no latch' '' "$(latch_of unreadable)"

assert_eq 'a rebuild latches' '1' "$(run_case both 0 "${SIGNAL_LINE}")"
assert_eq 'an unreadable journal then outranks the latch' '2' "$(run_case both 1)"
assert_succeeds 'but the latch is still reported' \
  grep -qF 'MAILBOX CACHE REBUILD DETECTED' "${WORK}/both.stderr"

printf '\n== invocation\n'

assert_eq 'a missing state dir is a usage error, not a silent pass' '1' \
  "$(
    status=0
    env -u BICHON_UIDVALIDITY_STATE_DIR bash "${SCRIPT}" >/dev/null 2>&1 || status=$?
    printf '%s' "${status}"
  )"

assert_succeeds 'the journal is read from a cursor file' \
  grep -qF -- "--cursor-file=${WORK}/state/detect/cursor" "${STUB_ARGS}"

assert_succeeds 'the read is scoped to one unit' \
  grep -qF -- '--unit=bichon.service' "${STUB_ARGS}"

assert_eq 'the watched unit is overridable' '0' \
  "$(watch_unit override bichon-test.service)"

assert_succeeds 'and the override reaches journalctl' \
  grep -qF -- '--unit=bichon-test.service' "${STUB_ARGS}"

report 'bichon-uidvalidity-watch'
