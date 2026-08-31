#!/usr/bin/env bash
#
# tests/github-machine-user.test.sh
#
# Unit tests for the fleet GitHub machine-user provisioning wizard (#745,
# ADR-0054). No real GitHub, no real password store: `gh` and `pa` are stubs
# on PATH whose behavior each case stages.
#
# The properties worth pinning are the ones the machine-user posture depends
# on: the wizard runs as the OWNER and refuses to act as the bot (honest
# review boundary, blast-radius containment); it invites each allowlisted repo
# at the requested least-privilege permission and nothing wider; and its
# verification proves the stored token authenticates as the bot and reaches
# every allowlisted repo — a token that authenticates as someone else, or a
# repo the bot cannot see, is a finding, not success.
#
# Run: ./tests/github-machine-user.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly SCRIPT="${REPO_ROOT}/examples/github-machine-user.sh"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

readonly BIN="${WORK}/bin"
readonly GH_ARGS="${WORK}/gh.args"
readonly ACTIVE_LOGIN="${WORK}/active.login"
readonly TOKEN_LOGIN="${WORK}/token.login"
readonly INVITE_RC="${WORK}/invite.rc"
readonly REACHABLE="${WORK}/reachable.repos"
readonly PA_TOKEN="${WORK}/pa.token"
readonly PA_ARGS="${WORK}/pa.args"

mkdir -p "${BIN}"

# gh stub. The active account (owner-guard) and the token's account (verify)
# are distinguished by GH_TOKEN being set on the verify calls.
cat >"${BIN}/gh" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${GH_ARGS}"
case "\$2" in
  user)
    if [ -n "\${GH_TOKEN:-}" ]; then cat "${TOKEN_LOGIN}"; else cat "${ACTIVE_LOGIN}"; fi
    ;;
  --method)
    exit "\$(cat "${INVITE_RC}")"
    ;;
  repos/*)
    if grep -qxF "\$2" "${REACHABLE}"; then printf '%s\n' "\${2#repos/}"; else exit 1; fi
    ;;
esac
STUB
chmod 0755 "${BIN}/gh"

cat >"${BIN}/pa" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${PA_ARGS}"
case "\$1" in
  show) [ -s "${PA_TOKEN}" ] && cat "${PA_TOKEN}" || exit 1 ;;
esac
STUB
chmod 0755 "${BIN}/pa"

export PATH="${BIN}:${PATH}"

# stage <active-login> <token-login> <invite-rc> <pa-token> <reachable-repo…>
stage() {
  printf '%s' "$1" >"${ACTIVE_LOGIN}"
  printf '%s' "$2" >"${TOKEN_LOGIN}"
  printf '%s' "$3" >"${INVITE_RC}"
  printf '%s' "$4" >"${PA_TOKEN}"
  shift 4
  : >"${REACHABLE}"
  local repo
  for repo in "$@"; do printf 'repos/%s\n' "${repo}" >>"${REACHABLE}"; done
  : >"${GH_ARGS}"
  : >"${PA_ARGS}"
}

# run <subcommand> — owner-driven defaults; each case overrides via env.
run() {
  local status=0
  env \
    FLEET_BOT_LOGIN="${FLEET_BOT_LOGIN-fleet-bot}" \
    FLEET_REPO_ALLOWLIST="${FLEET_REPO_ALLOWLIST-sripwoud/auberge sripwoud/dotfiles}" \
    FLEET_REPO_PERMISSION="${FLEET_REPO_PERMISSION-push}" \
    FLEET_PA_ENTRY="${FLEET_PA_ENTRY-fleet/github-pat}" \
    "${SCRIPT}" "$@" </dev/null >/dev/null 2>&1 || status=$?
  printf '%s' "${status}"
}

# ── checklist: pure narration, touches no external tool ──

stage owner fleet-bot 0 tok
assert_eq 'checklist exits 0' '0' "$(run checklist)"
assert_eq 'checklist is the default subcommand' '0' "$(run)"
assert_eq 'checklist calls gh not at all' '' "$(cat "${GH_ARGS}")"
assert_eq 'checklist names the pa entry and the fine-grained-token page' 'ok' "$(
  out="$(FLEET_PA_ENTRY='fleet/github-pat' "${SCRIPT}" checklist </dev/null 2>&1)"
  [[ "${out}" == *fleet/github-pat* && "${out}" == *personal-access-tokens* ]] && echo ok
)"

# ── usage: an unknown subcommand is an operational error ──

stage owner fleet-bot 0 tok
assert_eq 'unknown subcommand exits 2' '2' "$(run frobnicate)"

# ── invite: inputs are mandatory ──

stage owner fleet-bot 0 tok
assert_eq 'empty allowlist exits 2' '2' "$(FLEET_REPO_ALLOWLIST='' run invite)"
assert_eq 'empty allowlist touches gh not at all' '' "$(cat "${GH_ARGS}")"

stage owner fleet-bot 0 tok
assert_eq 'empty bot login exits 2' '2' "$(FLEET_BOT_LOGIN='' run invite)"

# ── invite: the owner guard — never act as the bot ──

stage fleet-bot fleet-bot 0 tok
assert_eq 'active account == bot exits 2' '2' "$(run invite)"
assert_eq 'owner guard blocks every invite' '0' "$(grep -c -- '--method PUT' "${GH_ARGS}" || true)"

# ── invite: happy path — one least-privilege invite per repo ──

stage owner fleet-bot 0 tok
assert_eq 'invite exits 0' '0' "$(run invite)"
assert_eq 'invites auberge at push' '1' \
  "$(grep -c -- 'api --method PUT repos/sripwoud/auberge/collaborators/fleet-bot -f permission=push' "${GH_ARGS}")"
assert_eq 'invites dotfiles at push' '1' \
  "$(grep -c -- 'api --method PUT repos/sripwoud/dotfiles/collaborators/fleet-bot -f permission=push' "${GH_ARGS}")"

# ── invite: the requested permission is honored verbatim ──

stage owner fleet-bot 0 tok
assert_eq 'triage permission is passed through' '1' \
  "$(
    FLEET_REPO_PERMISSION='triage' FLEET_REPO_ALLOWLIST='sripwoud/auberge' run invite >/dev/null
    grep -c -- '-f permission=triage' "${GH_ARGS}"
  )"

# ── invite: a failed invitation is an operational error ──

stage owner fleet-bot 1 tok
assert_eq 'a failed invite exits 2' '2' "$(run invite)"

# ── verify: happy path — token is the bot and reaches every repo ──

stage owner fleet-bot 0 tok sripwoud/auberge sripwoud/dotfiles
assert_eq 'verify exits 0' '0' "$(run verify)"
assert_eq 'verify reads the token from pa' '1' "$(grep -c -- 'show fleet/github-pat' "${PA_ARGS}")"

# ── verify: a token that authenticates as someone else is a finding ──

stage owner someone-else 0 tok sripwoud/auberge sripwoud/dotfiles
assert_eq 'wrong token identity exits 1' '1' "$(run verify)"

# ── verify: a repo the bot cannot reach is a finding ──

stage owner fleet-bot 0 tok sripwoud/auberge
assert_eq 'unreachable repo exits 1' '1' "$(run verify)"

# ── verify: a missing pa entry is an operational error ──

stage owner fleet-bot 0 '' sripwoud/auberge sripwoud/dotfiles
assert_eq 'missing pa entry exits 2' '2' "$(run verify)"

report 'github-machine-user'
