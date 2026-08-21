#!/usr/bin/env bash
#
# Nightly offsite backup of Immich to its dedicated B2 restic repository
# (#558). Zero downtime, upstream's own procedure: pg_dumpall on the live
# containerized database is MVCC-consistent, so the stack stays up while the
# dump and the photo library go out in one snapshot. Any failure fails the
# systemd unit, which is the Cockpit surface.
#
# Runs as root on the Immich Host with a crippled B2 application key — no
# deleteFiles capability. restic's unversioned deletes (lock cleanup) map to
# B2 hides, which need only writeFiles, so backup completes while a
# compromised box can add but never destroy. Retention (`forget`/`prune`) runs
# from the laptop with the full key: see examples/immich-b2-prune.sh.
#
# Configuration comes from the environment (the unit's EnvironmentFile); a
# manual seed run is `systemctl start immich-backup.service`.

set -euo pipefail

: "${RESTIC_REPOSITORY:?required}"
: "${RESTIC_PASSWORD:?required}"
: "${AWS_ACCESS_KEY_ID:?required}"
: "${AWS_SECRET_ACCESS_KEY:?required}"
: "${IMMICH_DB_CONTAINER:?required}"
: "${IMMICH_DB_DUMP_DIR:?required}"
: "${IMMICH_UPLOAD_DIR:?required}"

readonly DUMP_PATH="${IMMICH_DB_DUMP_DIR}/immich-db.sql.gz"

log() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >&2
}

# Stages through mktemp and publishes with mv, so a failed dump never replaces
# the previous night's good one — pipefail carries a pg_dumpall failure
# through gzip.
dump_database() {
  local tmp
  tmp=$(mktemp "${DUMP_PATH}.XXXXXX")
  if ! docker exec "${IMMICH_DB_CONTAINER}" \
    pg_dumpall --clean --if-exists --username=postgres \
    | gzip >"${tmp}"; then
    rm -f "${tmp}"
    return 1
  fi
  mv "${tmp}" "${DUMP_PATH}"
  log "dumped database to ${DUMP_PATH} ($(stat -c '%s' "${DUMP_PATH}") bytes)"
}

main() {
  log "dumping database from ${IMMICH_DB_CONTAINER}"
  dump_database
  log "pushing snapshot to ${RESTIC_REPOSITORY}"
  # The dump file, not its directory: a staging file orphaned by a hard kill
  # must never ride into a snapshot.
  restic backup "${DUMP_PATH}" "${IMMICH_UPLOAD_DIR}"
  log 'backup complete'
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
