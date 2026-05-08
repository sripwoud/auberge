#!/usr/bin/env bash
#
# check-stdout-discipline.sh — enforce clig.dev stdout-discipline rule.
#
# Rule: stdout = data, stderr = chrome (https://clig.dev/#output).
# println!/print! are only allowed in the modules that emit a command's primary
# data output.  All other modules must use eprintln!/eprint! or output::info/warn.
#
# Exit codes:
#   0 — no violations
#   1 — violations found
#   2 — self-test failed (regex is broken; file a bug)
#
# Usage:
#   ./.github/scripts/check-stdout-discipline.sh
#   GITHUB_ACTIONS=true ./.github/scripts/check-stdout-discipline.sh

set -euo pipefail

# Word-boundary anchor ensures eprintln!/eprint! do NOT match.
readonly PATTERN='\b(println|print)!'

# Trailing ':' anchors against the path-delimiter in `grep -n` output
# (path:line:content), so e.g. src/output.rs.bak is NOT allowlisted.
readonly ALLOWLIST='src/output\.rs:|src/commands/((backup|config_cmd|dns|headscale|host|select)|bichon/reconcile)\.rs:'

self_test() {
  echo 'println!()' | grep -qE "${PATTERN}" ||
    {
      echo "self-test FAILED: regex did not match println!"
      exit 2
    }
  echo 'print!()' | grep -qE "${PATTERN}" ||
    {
      echo "self-test FAILED: regex did not match print!"
      exit 2
    }
  if echo 'eprintln!()' | grep -qE "${PATTERN}"; then
    echo "self-test FAILED: regex false-matched eprintln!"
    exit 2
  fi
  if echo 'eprint!()' | grep -qE "${PATTERN}"; then
    echo "self-test FAILED: regex false-matched eprint!"
    exit 2
  fi
}

emit_error() {
  local message="${1}"
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::error::${message}" >&2
  else
    echo "error: ${message}" >&2
  fi
}

main() {
  self_test

  local hits
  hits=$(grep -rnE "${PATTERN}" src/ --include='*.rs' | grep -vE "${ALLOWLIST}" || true)

  if [[ -n "${hits}" ]]; then
    emit_error "println!/print! found outside data-emitting modules (use eprintln!/eprint! or output::info/warn for chrome):"
    echo "${hits}" >&2
    exit 1
  fi
}

main "$@"
