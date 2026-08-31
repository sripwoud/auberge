#!/usr/bin/env bash
#
# examples/github-machine-user.sh
#
# REFERENCE SCRIPT — NOT SHIPPED IN THE auberge BINARY
# ---------------------------------------------------
# Provisions the fleet's GitHub identity as a machine user, not the personal
# account (#745, ADR-0054): a leaked box credential never acts as the owner or
# touches the org, the bot cannot self-approve as the owner, and the token has
# its own lifecycle. GitHub ToS permits one such machine account.
#
# Account creation and fine-grained-token minting live behind a login only a
# human holds, so those stay a checklist; the parts a script can do without
# guessing — inviting each allowlisted repo at least privilege, and proving the
# stored token is the bot and reaches every repo — are subcommands the
# checklist tells you to run at the right moment.
#
#   github-machine-user.sh                 print the ordered checklist
#   github-machine-user.sh invite          invite the bot to every allowlist repo
#   github-machine-user.sh verify          prove the stored token is the bot
#
# The token never passes through this script: the human stores it with
# `pa add`, and `verify` reads it back through `pa show`. The script holds no
# secret and could be committed, run, and logged without leaking one.
#
# Configuration — the environment is the whole surface:
#
#   FLEET_BOT_LOGIN       the machine account's handle (required by invite/verify)
#   FLEET_REPO_ALLOWLIST  space-separated owner/repo list, least privilege per
#                         repo (required by invite/verify)
#   FLEET_REPO_PERMISSION collaborator permission for invite; default `push`,
#                         the least that lets the bot push a branch and open a
#                         PR — pull/triage cannot push, maintain/admin exceed it
#   FLEET_PA_ENTRY        pa entry the token is stored under; default
#                         `fleet/github-pat`, referenced from config.toml as
#                         `!pa show fleet/github-pat` and templated onto ruche
#                         by its meta role (#743)
#
# `invite` must run as the OWNER, authenticated in `gh`, holding admin on each
# allowlist repo. It refuses to run when the active `gh` account is the bot:
# the whole point is that the bot never provisions itself.
#
# The fine-grained token, minted in the checklist, is scoped to exactly the
# allowlist repos with the minimum permissions the fleet's push-and-PR flow
# needs:
#
#   Repository access  Only select repositories → the allowlist, nothing else
#   Contents           Read and write   (clone, push branches)
#   Pull requests      Read and write   (open and update PRs)
#   Metadata           Read-only        (mandatory, added automatically)
#
# Rotation — fine-grained tokens expire (90 days is the sane ceiling). To
# rotate: regenerate on the same token page, `pa edit fleet/github-pat` to
# store the new value, rerun `verify`, then `auberge deploy ruche` to
# re-template it onto the box. The old token dies on its expiry or on an
# explicit delete; nothing else changes.
#
# Prerequisites: gh (authenticated as the owner), pa.
#
# Exit codes:
#   0 — the requested phase completed
#   1 — verification finding: the stored token is not the bot, or the bot
#       cannot reach an allowlisted repo (the setup is not correct yet)
#   2 — operational error: missing prerequisite or input, unknown subcommand,
#       the owner guard tripped, or a gh/pa call failed
#
# shellcheck shell=bash

set -euo pipefail

readonly PROGRAM_NAME="${0##*/}"
readonly FLEET_REPO_PERMISSION="${FLEET_REPO_PERMISSION:-push}"
readonly FLEET_PA_ENTRY="${FLEET_PA_ENTRY:-fleet/github-pat}"
readonly TOKEN_PAGE='https://github.com/settings/personal-access-tokens/new'
readonly SIGNUP_PAGE='https://github.com/signup'

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

check_prerequisites() {
  local tool missing=()
  for tool in "$@"; do
    command -v "${tool}" >/dev/null 2>&1 || missing+=("${tool}")
  done
  ((${#missing[@]} == 0)) \
    || die 2 "not on PATH: ${missing[*]}" \
      'see the Prerequisites block at the top of this script'
}

require_inputs() {
  [[ -n "${FLEET_BOT_LOGIN:-}" ]] \
    || die 2 'FLEET_BOT_LOGIN must be set to the machine account handle'
  [[ -n "${FLEET_REPO_ALLOWLIST:-}" ]] \
    || die 2 'FLEET_REPO_ALLOWLIST must list at least one owner/repo'
}

# The bot never provisions itself: a self-invite would defeat both the honest
# review boundary and the blast-radius containment the machine user exists for.
owner_guard() {
  local active
  active=$(gh api user --jq .login) \
    || die 2 'could not read the active gh account' \
      'authenticate as the owner: gh auth login'
  [[ "${active}" != "${FLEET_BOT_LOGIN}" ]] \
    || die 2 "the active gh account is the bot (${active})" \
      'invite must run as the owner, who holds admin on the allowlist repos'
}

invite_repos() {
  local repo
  for repo in ${FLEET_REPO_ALLOWLIST}; do
    gh api --method PUT "repos/${repo}/collaborators/${FLEET_BOT_LOGIN}" \
      -f "permission=${FLEET_REPO_PERMISSION}" \
      || die 2 "could not invite ${FLEET_BOT_LOGIN} to ${repo}" \
        'the owner must hold admin on the repo; check the handle spelling'
    printf '%s: invited %s to %s (%s)\n' \
      "${PROGRAM_NAME}" "${FLEET_BOT_LOGIN}" "${repo}" "${FLEET_REPO_PERMISSION}"
  done
  printf '%s: the bot must now accept each invitation while logged in as itself\n' \
    "${PROGRAM_NAME}" >&2
}

verify() {
  local token login repo
  token=$(pa show "${FLEET_PA_ENTRY}") \
    || die 2 "no token stored at pa entry '${FLEET_PA_ENTRY}'" \
      'mint the fine-grained token (see the checklist) and store it:' \
      "  pa add ${FLEET_PA_ENTRY}"

  login=$(GH_TOKEN="${token}" gh api user --jq .login) \
    || die 2 'the stored token could not authenticate against GitHub' \
      'it may be expired or malformed — regenerate and re-store it'
  [[ "${login}" == "${FLEET_BOT_LOGIN}" ]] \
    || die 1 "the stored token authenticates as ${login}, not ${FLEET_BOT_LOGIN}" \
      'the wrong token is in the pa entry — store the machine user'"'"'s token'

  for repo in ${FLEET_REPO_ALLOWLIST}; do
    GH_TOKEN="${token}" gh api "repos/${repo}" --jq .full_name >/dev/null 2>&1 \
      || die 1 "the bot cannot reach ${repo}" \
        'the invitation is unaccepted, or the token is not scoped to this repo'
    printf '%s: %s reaches %s\n' "${PROGRAM_NAME}" "${FLEET_BOT_LOGIN}" "${repo}"
  done
}

print_checklist() {
  cat <<CHECKLIST
Fleet GitHub machine user — provisioning checklist (ADR-0054)

1. Create the machine account (browser, logged out of the owner account):
     ${SIGNUP_PAGE}
   Pick the handle and set FLEET_BOT_LOGIN to it. GitHub ToS permits one
   machine account alongside your personal one; give it its own email.

2. As the OWNER, invite the bot to the allowlist repos, least privilege:
     FLEET_BOT_LOGIN=<handle> \\
     FLEET_REPO_ALLOWLIST='owner/repo owner/other' \\
     ${PROGRAM_NAME} invite

3. As the BOT, accept each repository invitation:
     https://github.com/notifications  (or the emailed invite link)

4. As the BOT, mint a fine-grained personal access token:
     ${TOKEN_PAGE}
   Resource owner: the bot itself. Repository access: Only select
   repositories → the allowlist. Permissions:
     Contents       Read and write
     Pull requests  Read and write
     Metadata       Read-only (added automatically)
   Set an expiry (90 days is the sane ceiling) and note the rotation date.

5. Store the token in pa (the token never passes through this script):
     pa add ${FLEET_PA_ENTRY}

6. Prove the stored token is the bot and reaches every repo:
     FLEET_BOT_LOGIN=<handle> \\
     FLEET_REPO_ALLOWLIST='owner/repo owner/other' \\
     ${PROGRAM_NAME} verify

7. Reference it from config.toml so ruche's meta role templates it (#743):
     ruche_github_token = "!pa show ${FLEET_PA_ENTRY}"
CHECKLIST
}

main() {
  case "${1:-checklist}" in
    checklist)
      print_checklist
      ;;
    invite)
      check_prerequisites gh
      require_inputs
      owner_guard
      invite_repos
      ;;
    verify)
      check_prerequisites gh pa
      require_inputs
      verify
      ;;
    *)
      die 2 "unknown subcommand: $1" \
        "usage: ${PROGRAM_NAME} [checklist|invite|verify]"
      ;;
  esac
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
