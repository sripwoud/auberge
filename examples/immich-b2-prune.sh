#!/usr/bin/env bash
#
# examples/immich-b2-prune.sh
#
# REFERENCE SCRIPT — NOT SHIPPED IN THE auberge BINARY
# ---------------------------------------------------
# The laptop half of the Immich offsite backup (#558). The Immich Host backs
# up nightly to B2 with a crippled application key that cannot delete, so
# retention has to run where the full key lives: here, weekly, as a systemd
# user timer.
#
# Two phases, in order:
#
#   1. watchdog   newest snapshot older than 48h — or an empty or unreadable
#                 repository — raises a desktop notification and exits 1, a
#                 failed user unit. Retention does not run: pruning a repo the
#                 box has stopped writing to only walks toward destroying the
#                 history that is left.
#   2. retention  restic forget --keep-within 30d --keep-monthly 12 --prune,
#                 per restic's append-only guidance.
#
# Configuration — the environment is the whole surface. restic reads its own
# variables: RESTIC_REPOSITORY, RESTIC_PASSWORD (or RESTIC_PASSWORD_COMMAND /
# RESTIC_PASSWORD_FILE), and whatever the backend needs — for B2's S3 API,
# AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY. This script assumes an offsite
# repository already exists and the environment can reach it; it deliberately
# knows nothing about any secret store or config tool. The credentials it runs
# with must be the FULL key — the one with deleteFiles — never the box key:
# retention against the box key fails, which is exactly the capability split
# #558 exists to prove. The 48h threshold is overridable via
# IMMICH_SNAPSHOT_MAX_AGE_HOURS.
#
# Install on the laptop — a wrapper owns provisioning the environment, so the
# unit file holds no credentials and this script stays tool-agnostic:
#
#   cp examples/immich-b2-prune.sh ~/.local/bin/immich-b2-prune
#
#   ~/.local/bin/immich-b2-prune-wrapped (0700), adapted to your own tooling:
#     #!/usr/bin/env bash
#     set -euo pipefail
#     export RESTIC_REPOSITORY='s3:https://s3.example.com/my-immich-bucket'
#     export RESTIC_PASSWORD="$(your-secret-tool get immich-restic-password)"
#     export AWS_ACCESS_KEY_ID="$(your-secret-tool get immich-b2-key-id)"
#     export AWS_SECRET_ACCESS_KEY="$(your-secret-tool get immich-b2-key)"
#     exec "${HOME}/.local/bin/immich-b2-prune"
#
#   ~/.config/systemd/user/immich-b2-prune.service:
#     [Unit]
#     Description=Immich B2 retention and staleness watchdog
#     [Service]
#     Type=oneshot
#     ExecStart=%h/.local/bin/immich-b2-prune-wrapped
#
#   ~/.config/systemd/user/immich-b2-prune.timer:
#     [Unit]
#     Description=Run the Immich B2 retention weekly
#     [Timer]
#     OnCalendar=weekly
#     RandomizedDelaySec=1h
#     Persistent=true
#     [Install]
#     WantedBy=timers.target
#
#   systemctl --user daemon-reload
#   systemctl --user enable --now immich-b2-prune.timer
#
# Verify the watchdog once by pointing RESTIC_REPOSITORY at an empty or stale
# repository and watching the run fail with a notification.
#
# Prerequisites: restic, jq, GNU date.
#
# Exit codes:
#   0 — backup fresh, retention applied
#   1 — watchdog finding (stale or empty repository) or retention failed
#   2 — operational error: missing prerequisite, credential resolution failed,
#       or the repository could not be read (distinguishable from "found
#       stale", the way the fleet's other watchers keep the two apart)
#
# shellcheck shell=bash

set -euo pipefail

readonly PROGRAM_NAME="${0##*/}"
readonly KEEP_WITHIN='30d'
readonly KEEP_MONTHLY=12
readonly MAX_SNAPSHOT_AGE_HOURS="${IMMICH_SNAPSHOT_MAX_AGE_HOURS:-48}"

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

# The failed user unit is the durable signal; the notification is the one a
# desktop actually surfaces. Its delivery must not mask the message, so it is
# best-effort by design.
alert() {
  local message="$1"
  if command -v notify-send >/dev/null 2>&1; then
    notify-send --urgency=critical 'immich offsite backup' "${message}" || true
  fi
  printf '%s: %s\n' "${PROGRAM_NAME}" "${message}" >&2
}

check_prerequisites() {
  local tool missing=()
  for tool in restic jq date; do
    command -v "${tool}" >/dev/null 2>&1 || missing+=("${tool}")
  done
  ((${#missing[@]} == 0)) \
    || die 2 "not on PATH: ${missing[*]}" \
      'see the Prerequisites block at the top of this script'

  [[ "${MAX_SNAPSHOT_AGE_HOURS}" =~ ^[1-9][0-9]*$ ]] \
    || die 2 "IMMICH_SNAPSHOT_MAX_AGE_HOURS must be a positive integer, got: ${MAX_SNAPSHOT_AGE_HOURS}"
}

# Which tool provisions the environment is the wrapper's business (see the
# Install block above); guessing one here would quietly couple every operator
# to it. Only the repository location is checked — restic's own complaint for
# it is cryptic, while a missing password or backend credential varies by
# backend and surfaces through the watchdog as an unreadable repository.
require_repository() {
  [[ -n "${RESTIC_REPOSITORY:-}" ]] \
    || die 2 'RESTIC_REPOSITORY must be set in the environment' \
      'this script assumes an offsite restic repository already exists;' \
      'the wrapper that launches it provisions the environment (see Install' \
      'in the header)'
}

# Newest snapshot time, taken as the lexicographic max of the RFC 3339 strings
# rather than parsed per entry: mixed UTC offsets can mis-order by at most the
# offset delta, noise against a 48h threshold.
watchdog() {
  local snapshots_json
  # exit 2, not 1: an unreadable repository is an operational error, not a
  # staleness finding — but it blocks retention all the same, and the operator
  # still gets woken up.
  if ! snapshots_json=$(restic snapshots --json); then
    alert 'cannot read the B2 repository — the offsite backup cannot be vouched for'
    exit 2
  fi

  local newest
  newest=$(printf '%s' "${snapshots_json}" | jq -r 'map(.time) | max // empty') \
    || die 2 'restic snapshots returned JSON this script cannot read'

  if [[ -z "${newest}" ]]; then
    alert 'the B2 repository holds no snapshots — the Immich box has never backed up'
    exit 1
  fi

  local newest_epoch age_hours
  newest_epoch=$(date -d "${newest}" +%s) \
    || die 2 "cannot parse snapshot time: ${newest}"
  age_hours=$((($(date +%s) - newest_epoch) / 3600))

  if ((age_hours > MAX_SNAPSHOT_AGE_HOURS)); then
    alert "newest snapshot is ${age_hours}h old (limit ${MAX_SNAPSHOT_AGE_HOURS}h) — the Immich box has stopped backing up"
    exit 1
  fi
}

apply_retention() {
  restic forget --keep-within "${KEEP_WITHIN}" --keep-monthly "${KEEP_MONTHLY}" --prune \
    || die 1 'retention failed' \
      'a stale lock from an interrupted run is the usual cause: restic unlock' \
      'a permission error means the box key leaked into this half — it must' \
      'never be able to prune'
}

main() {
  check_prerequisites
  require_repository
  watchdog
  apply_retention
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
