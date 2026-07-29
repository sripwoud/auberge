#!/usr/bin/env bash
#
# check-adr-numbering.sh — one ADR number, one file, all indexed.
#
# Two ADR-0007 files were committed in parallel and diverged for ~3 months
# (2026-05-07 to 2026-07-29); the docs linked the copy that was missing an
# amendment. Nothing detected it because nothing asserted these invariants:
#
#   1. Each NNNN prefix in meta/adr/ belongs to exactly one file.
#   2. Every meta/adr/*.md is linked from the meta/adr.md index.
#   3. Every ./adr/ link in the index resolves to a file that exists.
#
# Exit codes:
#   0 — invariants hold
#   1 — violations found
#
# Usage:
#   ./.github/scripts/check-adr-numbering.sh
#   GITHUB_ACTIONS=true ./.github/scripts/check-adr-numbering.sh

set -euo pipefail

readonly ADR_DIR='meta/adr'
readonly INDEX='meta/adr.md'

emit_error() {
  local message="${1}"
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::error::${message}" >&2
  else
    echo "error: ${message}" >&2
  fi
}

check_unique_numbers() {
  local dupes
  dupes=$(
    find "${ADR_DIR}" -maxdepth 1 -name '[0-9][0-9][0-9][0-9]-*.md' -printf '%f\n' \
      | cut -c1-4 | sort | uniq -d
  )

  [[ -z "${dupes}" ]] && return 0

  local number
  while read -r number; do
    emit_error "ADR number ${number} is used by more than one file:"
    find "${ADR_DIR}" -maxdepth 1 -name "${number}-*.md" -printf '  %p\n' >&2
  done <<<"${dupes}"
  return 1
}

check_all_indexed() {
  local status=0 file
  while read -r file; do
    grep -qF "(./adr/${file})" "${INDEX}" \
      || {
        emit_error "${ADR_DIR}/${file} is not linked from ${INDEX}"
        status=1
      }
  done < <(find "${ADR_DIR}" -maxdepth 1 -name '[0-9][0-9][0-9][0-9]-*.md' -printf '%f\n')
  return "${status}"
}

check_index_links_resolve() {
  local status=0 target
  while read -r target; do
    [[ -f "${ADR_DIR}/${target}" ]] \
      || {
        emit_error "${INDEX} links ./adr/${target}, which does not exist"
        status=1
      }
  done < <(grep -oP '\(\./adr/\K[^)]+' "${INDEX}" || true)
  return "${status}"
}

main() {
  local status=0
  check_unique_numbers || status=1
  check_all_indexed || status=1
  check_index_links_resolve || status=1
  exit "${status}"
}

main "$@"
