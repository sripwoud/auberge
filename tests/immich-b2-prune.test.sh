#!/usr/bin/env bash
#
# tests/immich-b2-prune.test.sh
#
# Unit tests for the laptop half of the Immich offsite backup (#558): the
# weekly watchdog + retention script that holds the only key allowed to
# destroy repository data.
#
# No restic, no B2, no pa, no auberge — all stubs on PATH whose behavior each
# case stages (jq, date, and dasel are real). The properties worth pinning are
# the ones the append-only posture depends on: that a stale, empty, or
# unreadable repository raises the alarm and blocks retention (pruning a repo
# the box has stopped writing to would destroy the only history left), and
# that the full credentials come out of the operator's pa store — one
# decryption, both fields — rather than living in the unit file.
#
# Run: ./tests/immich-b2-prune.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly SCRIPT="${REPO_ROOT}/examples/immich-b2-prune.sh"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

readonly BIN="${WORK}/bin"
readonly RESTIC_ARGS="${WORK}/restic.args"
readonly SNAPSHOTS_OUT="${WORK}/snapshots.out"
readonly SNAPSHOTS_RC="${WORK}/snapshots.rc"
readonly FORGET_RC="${WORK}/forget.rc"
readonly FORGET_SEEN_PASSWORD="${WORK}/forget.seen-password"
readonly FORGET_SEEN_AWS_KEY="${WORK}/forget.seen-aws-key"
readonly PA_ARGS="${WORK}/pa.args"
readonly PA_RC="${WORK}/pa.rc"
readonly PA_YAML="${WORK}/pa.yaml"
readonly AUBERGE_ARGS="${WORK}/auberge.args"
readonly NOTIFY_ARGS="${WORK}/notify.args"

mkdir -p "${BIN}"

cat >"${BIN}/restic" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${RESTIC_ARGS}"
case "\$1" in
  snapshots)
    rc="\$(cat "${SNAPSHOTS_RC}")"
    if [ "\${rc}" -ne 0 ]; then exit "\${rc}"; fi
    cat "${SNAPSHOTS_OUT}"
    ;;
  forget)
    printf '%s\n' "\${RESTIC_PASSWORD:-}" >"${FORGET_SEEN_PASSWORD}"
    printf '%s\n' "\${AWS_SECRET_ACCESS_KEY:-}" >"${FORGET_SEEN_AWS_KEY}"
    exit "\$(cat "${FORGET_RC}")"
    ;;
esac
STUB
chmod 0755 "${BIN}/restic"

# The backblaze entry's shape: first line is the account password (tail'ed
# away by the script), the rest is YAML keyed by application key name. The
# keyID is YAML-quoted deliberately: an all-digit id would otherwise parse as
# a number, and the script must hand restic the string.
cat >"${BIN}/pa" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${PA_ARGS}"
if [ "\$(cat "${PA_RC}")" -ne 0 ]; then exit 1; fi
[ "\$1" = 'show' ] && [ "\$2" = 'backblaze' ] || exit 1
cat "${PA_YAML}"
STUB
chmod 0755 "${BIN}/pa"

stage_pa_yaml() {
  cat >"${PA_YAML}" <<'YAML'
account-web-password
applicationKeys:
  master:
    keyID: "0012master"
    applicationKey: master-secret
  immich-laptop:
    keyID: "0034full"
    applicationKey: full-key-secret
YAML
}

cat >"${BIN}/auberge" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${AUBERGE_ARGS}"
case "\$3" in
  immich_restic_repository) printf 's3:https://s3.example.test/immich\n' ;;
  immich_restic_password) printf 'repo-pass\n' ;;
  *) exit 1 ;;
esac
STUB
chmod 0755 "${BIN}/auberge"

cat >"${BIN}/notify-send" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${NOTIFY_ARGS}"
STUB
chmod 0755 "${BIN}/notify-send"

export PATH="${BIN}:${PATH}"

# stage <snapshots-rc> <forget-rc> <snapshot-time…>
# Each snapshot time becomes one entry in the staged `restic snapshots --json`
# listing; none stages an empty repository.
stage() {
  local snapshots_rc="$1" forget_rc="$2"
  shift 2
  printf '%s' "${snapshots_rc}" >"${SNAPSHOTS_RC}"
  printf '%s' "${forget_rc}" >"${FORGET_RC}"
  printf '0' >"${PA_RC}"
  stage_pa_yaml
  local time entries=()
  for time in "$@"; do entries+=("{\"time\": \"${time}\"}"); done
  printf '[%s]\n' "$(
    IFS=,
    printf '%s' "${entries[*]-}"
  )" >"${SNAPSHOTS_OUT}"
  : >"${RESTIC_ARGS}"
  : >"${PA_ARGS}"
  : >"${AUBERGE_ARGS}"
  : >"${NOTIFY_ARGS}"
  : >"${FORGET_SEEN_PASSWORD}"
  : >"${FORGET_SEEN_AWS_KEY}"
}

run_prune() {
  local status=0
  "${SCRIPT}" >/dev/null 2>&1 || status=$?
  printf '%s' "${status}"
}

hours_ago() {
  date -u -d "-$1 hours" '+%Y-%m-%dT%H:%M:%SZ'
}

# ── fresh repository: retention runs, nobody is woken up ──

stage 0 0 "$(hours_ago 60)" "$(hours_ago 6)"
assert_eq 'fresh snapshot exits 0' '0' "$(run_prune)"
assert_eq 'watchdog reads the repo, then retention prunes it' \
  "snapshots --json
forget --keep-within 30d --keep-monthly 12 --prune" \
  "$(cat "${RESTIC_ARGS}")"
assert_eq 'no notification when the backup is healthy' '' "$(cat "${NOTIFY_ARGS}")"
assert_eq 'repository URL and password come from auberge config' \
  'config get immich_restic_repository
config get immich_restic_password' \
  "$(cat "${AUBERGE_ARGS}")"
assert_eq 'full B2 key comes from the backblaze pa entry, decrypted once' \
  'show backblaze' "$(cat "${PA_ARGS}")"
assert_eq 'restic sees the password from auberge config' \
  'repo-pass' "$(cat "${FORGET_SEEN_PASSWORD}")"
assert_eq 'restic sees the application key from the pa yaml' \
  'full-key-secret' "$(cat "${FORGET_SEEN_AWS_KEY}")"

# ── stale repository: alarm, and retention must not run ──

stage 0 0 "$(hours_ago 100)"
assert_eq 'stale snapshot exits 1' '1' "$(run_prune)"
assert_eq 'stale snapshot raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'stale snapshot blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

# ── empty repository: same alarm — there is nothing safe to prune ──

stage 0 0
assert_eq 'empty repository exits 1' '1' "$(run_prune)"
assert_eq 'empty repository raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'empty repository blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

# ── unreadable repository: an operational error, not a staleness finding ──

stage 1 0 "$(hours_ago 6)"
assert_eq 'unreadable repository exits 2' '2' "$(run_prune)"
assert_eq 'unreadable repository raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'unreadable repository blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

# ── credential resolution failure refuses the run before restic ──

stage 0 0 "$(hours_ago 6)"
printf '1' >"${PA_RC}"
assert_eq 'pa failure exits 2' '2' "$(run_prune)"
assert_eq 'pa failure touches restic not at all' '' "$(cat "${RESTIC_ARGS}")"

# ── a yaml that lacks the key is the same refusal, not empty credentials ──

stage 0 0 "$(hours_ago 6)"
printf 'account-web-password\napplicationKeys:\n  master:\n    keyID: "0012master"\n' >"${PA_YAML}"
assert_eq 'missing applicationKeys entry exits 2' '2' "$(run_prune)"
assert_eq 'missing applicationKeys entry touches restic not at all' '' "$(cat "${RESTIC_ARGS}")"

# ── environment overrides skip auberge and pa entirely ──

stage 0 0 "$(hours_ago 6)"
printf '1' >"${PA_RC}"
status=0
env \
  RESTIC_REPOSITORY='s3:https://s3.example.test/other' \
  RESTIC_PASSWORD='override-pass' \
  AWS_ACCESS_KEY_ID='override-id' \
  AWS_SECRET_ACCESS_KEY='override-secret' \
  "${SCRIPT}" >/dev/null 2>&1 || status=$?
assert_eq 'env overrides exit 0 without pa or auberge' '0' "${status}"
assert_eq 'env overrides never call pa' '' "$(cat "${PA_ARGS}")"
assert_eq 'env overrides never call auberge' '' "$(cat "${AUBERGE_ARGS}")"
assert_eq 'env override password reaches restic' \
  'override-pass' "$(cat "${FORGET_SEEN_PASSWORD}")"

# ── a failed forget fails the unit ──

stage 0 1 "$(hours_ago 6)"
assert_eq 'failed forget exits 1' '1' "$(run_prune)"

report 'immich-b2-prune'
