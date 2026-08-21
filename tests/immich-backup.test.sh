#!/usr/bin/env bash
#
# tests/immich-backup.test.sh
#
# Unit tests for the on-host Immich offsite backup (#558): the nightly script
# that dumps the containerized database live and pushes it with the photo
# library to the dedicated B2 restic repository.
#
# No docker, no restic, no B2 — both are stubs on PATH whose behavior each case
# stages. The properties worth pinning are the ones a live run could not show
# on demand: that a failed dump never replaces the previous night's good one,
# that restic is not asked to snapshot a dump that failed, and that the
# credentials reach restic through the environment rather than argv.
#
# Run: ./tests/immich-backup.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly SCRIPT="${REPO_ROOT}/ansible/roles/immich/files/immich-backup.sh"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

readonly BIN="${WORK}/bin"
readonly DOCKER_ARGS="${WORK}/docker.args"
readonly DOCKER_RC="${WORK}/docker.rc"
readonly RESTIC_ARGS="${WORK}/restic.args"
readonly RESTIC_RC="${WORK}/restic.rc"
readonly RESTIC_SEEN_PASSWORD="${WORK}/restic.seen-password"
readonly DUMP_DIR="${WORK}/db-dump"
readonly UPLOAD_DIR="${WORK}/upload"

mkdir -p "${BIN}" "${DUMP_DIR}" "${UPLOAD_DIR}"

cat >"${BIN}/docker" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${DOCKER_ARGS}"
rc="\$(cat "${DOCKER_RC}")"
if [ "\${rc}" -ne 0 ]; then exit "\${rc}"; fi
printf 'CREATE TABLE immich_assets;\n'
STUB
chmod 0755 "${BIN}/docker"

cat >"${BIN}/restic" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${RESTIC_ARGS}"
printf '%s\n' "\${RESTIC_PASSWORD:-}" >"${RESTIC_SEEN_PASSWORD}"
exit "\$(cat "${RESTIC_RC}")"
STUB
chmod 0755 "${BIN}/restic"

export PATH="${BIN}:${PATH}"

stage() {
  local docker_rc="$1" restic_rc="$2"
  printf '%s' "${docker_rc}" >"${DOCKER_RC}"
  printf '%s' "${restic_rc}" >"${RESTIC_RC}"
  : >"${DOCKER_ARGS}"
  : >"${RESTIC_ARGS}"
  : >"${RESTIC_SEEN_PASSWORD}"
}

# Runs the script with the full required environment, printing its exit status.
run_backup() {
  local status=0
  env \
    RESTIC_REPOSITORY='s3:https://s3.example.test/immich' \
    RESTIC_PASSWORD='repo-pass' \
    AWS_ACCESS_KEY_ID='box-key-id' \
    AWS_SECRET_ACCESS_KEY='box-key-secret' \
    IMMICH_DB_CONTAINER='immich_postgres' \
    IMMICH_DB_DUMP_DIR="${DUMP_DIR}" \
    IMMICH_UPLOAD_DIR="${UPLOAD_DIR}" \
    "${SCRIPT}" >/dev/null 2>&1 || status=$?
  printf '%s' "${status}"
}

# ── a missing required variable refuses the run before any tool is touched ──

stage 0 0
status=$(
  env \
    RESTIC_REPOSITORY='s3:https://s3.example.test/immich' \
    AWS_ACCESS_KEY_ID='box-key-id' \
    AWS_SECRET_ACCESS_KEY='box-key-secret' \
    IMMICH_DB_CONTAINER='immich_postgres' \
    IMMICH_DB_DUMP_DIR="${DUMP_DIR}" \
    IMMICH_UPLOAD_DIR="${UPLOAD_DIR}" \
    "${SCRIPT}" >/dev/null 2>&1 || printf '%s' "$?"
)
assert_fails 'missing RESTIC_PASSWORD refuses the run' test "${status}" = '0'
assert_eq 'missing RESTIC_PASSWORD touches neither docker nor restic' \
  '' "$(cat "${DOCKER_ARGS}" "${RESTIC_ARGS}")"

# ── happy path: dump published, then one snapshot of dump dir + upload dir ──

stage 0 0
assert_eq 'happy path exits 0' '0' "$(run_backup)"
assert_eq 'database is dumped live via docker exec' \
  'exec immich_postgres pg_dumpall --clean --if-exists --username=postgres' \
  "$(cat "${DOCKER_ARGS}")"
assert_eq 'dump lands gzipped at its stable path' \
  'CREATE TABLE immich_assets;' \
  "$(gunzip -c "${DUMP_DIR}/immich-db.sql.gz")"
assert_eq 'restic snapshots the dump file and the upload dir together' \
  "backup ${DUMP_DIR}/immich-db.sql.gz ${UPLOAD_DIR}" \
  "$(cat "${RESTIC_ARGS}")"
assert_eq 'credentials reach restic through the environment' \
  'repo-pass' "$(cat "${RESTIC_SEEN_PASSWORD}")"

# ── a failed dump never replaces the previous good one, and restic never runs ──

stage 1 0
printf 'OLD GOOD DUMP\n' | gzip >"${DUMP_DIR}/immich-db.sql.gz"
assert_fails 'failed pg_dumpall fails the run' test "$(run_backup)" = '0'
assert_eq 'previous dump survives a failed pg_dumpall' \
  'OLD GOOD DUMP' "$(gunzip -c "${DUMP_DIR}/immich-db.sql.gz")"
assert_eq 'restic is not asked to snapshot a failed dump' '' "$(cat "${RESTIC_ARGS}")"
assert_eq 'no staging litter is left behind' \
  "${DUMP_DIR}/immich-db.sql.gz" \
  "$(find "${DUMP_DIR}" -type f | sort | paste -sd' ' -)"

# ── a restic failure fails the unit ──

stage 0 1
assert_fails 'failed restic backup fails the run' test "$(run_backup)" = '0'

report 'immich-backup'
