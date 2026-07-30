#!/usr/bin/env bash
#
# bichon-uidvalidity-watch
#
# Latches the one Bichon event that costs data silently. On a UIDVALIDITY
# mismatch Bichon calls rebuild_mailbox_cache, which deletes the folder's
# envelopes and their blobs from the Internal Store before refetching; mail
# already expunged from the Upstream Mailbox cannot be refetched, so its
# searchability and tags are gone until an operator notices and replays the
# Email Archive (ADR-0012, ADR-0014). Bichon logs the trigger at info level and
# carries on. Nothing else notices.
#
# Reads bichon.service's journal from a saved cursor and appends every match to
# a latch file. The unit fails while that file is non-empty, so the report is
# republished on every run and clears only when an operator deletes the file —
# the failed unit is the alert surface (systemctl --failed, Cockpit → Services).
#
# Environment:
#   BICHON_UIDVALIDITY_STATE_DIR  required — holds the cursor and the latch
#   BICHON_UIDVALIDITY_UNIT       unit to read (default bichon.service)
#
# Exit codes, matching `auberge backup verify`:
#   0 — no rebuild recorded
#   1 — a rebuild is recorded and unacknowledged
#   2 — operational error: the journal could not be read
#
# shellcheck shell=bash

set -euo pipefail

: "${BICHON_UIDVALIDITY_STATE_DIR:?required}"
: "${BICHON_UIDVALIDITY_UNIT:=bichon.service}"

# The coupling point to upstream, and the reason it is a fixed string rather
# than an operator knob: it must match what Bichon writes, not what an operator
# guesses. Preferred over the folder status Bichon also sets ("UID validity
# changed, rebuilding...") because only this one reaches the journal. Unchanged
# from the deployed 0.3.7 (src/modules/cache/imap/sync/flow.rs:283) through the
# workspace re-crate refactor that moved the same line to
# crates/core/src/cache/imap/download/flow.rs:491.
readonly SIGNAL='detected with changed uid_validity'

readonly CURSOR_FILE="${BICHON_UIDVALIDITY_STATE_DIR}/cursor"
readonly REBUILD_LOG="${BICHON_UIDVALIDITY_STATE_DIR}/rebuilds.log"
readonly SELF_UNIT='bichon-uidvalidity-watch.service'

log() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >&2
}

# journalctl advances the cursor file to the last entry it showed, so a run that
# reads nothing new still records where it stopped and no entry is read twice.
# Scoping to one unit also keeps this script's own output — which quotes SIGNAL
# back when it reports — from ever matching on a later run.
read_new_entries() {
  journalctl \
    --unit="${BICHON_UIDVALIDITY_UNIT}" \
    --cursor-file="${CURSOR_FILE}" \
    --output=short-iso-precise \
    --no-pager \
    --quiet
}

# The first run has no cursor and so reads the whole retained journal.
# Deliberate: a rebuild that already happened is precisely what the operator was
# never told about, and seeding a silent baseline would bury it.
#
# Only the matching line is kept. Bichon formats the uid values with {:#?},
# which spills them onto continuation lines; the mailbox name and the timestamp
# are both on the line that matches, and the discarded values name a generation
# counter nobody acts on.
record_rebuilds() {
  local entries matches
  if ! entries=$(read_new_entries); then
    log "journal read failed for unit ${BICHON_UIDVALIDITY_UNIT}"
    return 2
  fi

  matches=$(printf '%s\n' "${entries}" | grep -F "${SIGNAL}" || true)
  [[ -z "${matches}" ]] && return 0

  printf '%s\n' "${matches}" >>"${REBUILD_LOG}"
}

# Latched, not edge-triggered: systemd clears a unit's failed state on the next
# successful start, so a rebuild announced by exit code alone would be erased by
# the following hourly tick. The file is the durable signal; the exit code only
# republishes it.
report() {
  [[ -s "${REBUILD_LOG}" ]] || {
    log "no mailbox cache rebuild recorded"
    return 0
  }

  log "MAILBOX CACHE REBUILD DETECTED — the Internal Store dropped envelopes for:"
  while IFS= read -r line; do
    log "  ${line}"
  done <"${REBUILD_LOG}"
  log "Mail still on the Upstream Mailbox was refetched; mail already expunged there was not."
  log "Replay the Email Archive with examples/bichon-restore.sh, then acknowledge with:"
  log "  rm ${REBUILD_LOG} && systemctl start ${SELF_UNIT}"
  return 1
}

main() {
  mkdir -p "${BICHON_UIDVALIDITY_STATE_DIR}"

  local scan_status=0 report_status=0
  record_rebuilds || scan_status=$?
  report || report_status=$?

  # An unreadable journal outranks the latch: the run could not answer the
  # question at all, which is what exit 2 means everywhere else in auberge. The
  # latch is still printed above, so an operator loses no information to it.
  if [[ "${scan_status}" -ne 0 ]]; then
    return "${scan_status}"
  fi
  return "${report_status}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
