#!/usr/bin/env bash
#
# recompute-checksums.sh — recompute co-pinned sha256 literals after a bump.
#
# Renovate bumps a version variable but cannot know the new release asset's
# digest, and fetching an upstream checksum file is not an option: gokapi
# publishes none (its digest was computed by hand), and a checksum fetched
# alongside the asset proves nothing — an attacker who can swap the asset can
# swap the checksum file beside it. So this script downloads the asset and
# computes the digest itself.
#
# A digest variable <prefix>_sha256 / <prefix>_checksum pairs with the
# <prefix>_url variable in the same defaults file. The digest is recomputed
# only when the rendered URL changed against the base ref: the URL is a pure
# function of the file's own variables plus the App Version declared in the
# App's Playbook Meta (ADR-0017), so an unchanged URL means the digest must
# not move — its immutability after merge is what detects an upstream re-tag.
#
# Exit codes:
#   0 — every stale digest recomputed (files modified in place) or none stale
#   1 — a URL failed to render or an asset failed to download
#
# Usage:
#   ./.github/scripts/recompute-checksums.sh origin/master

set -euo pipefail

WORKDIR="$(mktemp -d)"
readonly WORKDIR
trap 'rm -rf "${WORKDIR}"' EXIT

emit_error() {
  local message="${1}"
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::error::${message}" >&2
  else
    echo "error: ${message}" >&2
  fi
}

# Substitute {{ var }} references with assignments found in the same file.
# The App Version is declared in the Playbook Meta rather than the defaults
# file (ADR-0017), so {{ <app>_version }} resolves from the meta's `value:`.
render_url() {
  local file="${1}" meta_file="${2}" app="${3}" value="${4}"
  local iterations=0 var sub
  while [[ "${value}" == *'{{'* ]]; do
    if ((iterations >= 10)); then
      emit_error "unresolved template after 10 passes: ${value}"
      return 1
    fi
    iterations=$((iterations + 1))
    var="$(grep -oP '\{\{ \K[a-z0-9_]+(?= \}\})' <<<"${value}" | head -1 || true)"
    if [[ -z "${var}" ]]; then
      emit_error "cannot parse template reference in: ${value}"
      return 1
    fi
    sub="$(grep -oP "^${var}: \"?\K[^\"]+" "${file}" | head -1 || true)"
    if [[ -z "${sub}" && "${var}" == "${app}_version" && -n "${meta_file}" ]]; then
      sub="$(grep -oP '^  value: "?\K[^"]+' "${meta_file}" | head -1 || true)"
    fi
    if [[ -z "${sub}" ]]; then
      emit_error "cannot resolve {{ ${var} }} from ${file}"
      return 1
    fi
    value="${value//"{{ ${var} }}"/${sub}}"
  done
  printf '%s\n' "${value}"
}

# Extract the raw (unrendered) value of a variable from a defaults file.
raw_value() {
  local file="${1}" var="${2}"
  grep -oP "^${var}: \"?\K[^\"]+" "${file}" | head -1 || true
}

recompute_digest() {
  local file="${1}" base_file="${2}" meta_file="${3}" base_meta_file="${4}"
  local app="${5}" var_name="${6}" old_digest="${7}"
  local url_var="${var_name%_*}_url"
  local head_tmpl head_url base_tmpl base_url new_digest asset

  head_tmpl="$(raw_value "${file}" "${url_var}")"
  if [[ -z "${head_tmpl}" ]]; then
    emit_error "${file}: ${var_name} has no paired ${url_var}"
    return 1
  fi
  head_url="$(render_url "${file}" "${meta_file}" "${app}" "${head_tmpl}")" || return 1

  if [[ -n "${base_file}" ]]; then
    base_tmpl="$(raw_value "${base_file}" "${url_var}")"
    if [[ -n "${base_tmpl}" ]]; then
      base_url="$(render_url "${base_file}" "${base_meta_file}" "${app}" "${base_tmpl}")" || base_url=""
      if [[ "${base_url}" == "${head_url}" ]]; then
        echo "${file}: ${url_var} unchanged, leaving ${var_name} alone"
        return 0
      fi
    fi
  fi

  asset="${WORKDIR}/asset"
  if ! curl -fsSL --retry 3 --retry-delay 5 -o "${asset}" "${head_url}"; then
    emit_error "${file}: failed to download ${head_url}"
    return 1
  fi
  new_digest="$(sha256sum "${asset}" | cut -d' ' -f1)"

  if [[ "${new_digest}" == "${old_digest}" ]]; then
    echo "${file}: ${var_name} already matches ${head_url}"
    return 0
  fi

  sed -i -E "s|^(${var_name}: .*)${old_digest}|\1${new_digest}|" "${file}"
  echo "${file}: ${var_name} ${old_digest:0:8}… -> ${new_digest:0:8}… (${head_url})"
}

process_file() {
  local base_ref="${1}" file="${2}"
  local status=0 app base_file meta_file base_meta_file line var_name old_digest

  app="$(basename "$(dirname "$(dirname "${file}")")")"

  base_file="${WORKDIR}/base-${app}"
  if ! git show "${base_ref}:${file}" >"${base_file}" 2>/dev/null; then
    base_file=""
  fi

  meta_file="ansible/playbooks/${app}.meta.yml"
  [[ -f "${meta_file}" ]] || meta_file=""

  base_meta_file="${WORKDIR}/base-meta-${app}"
  if ! git show "${base_ref}:ansible/playbooks/${app}.meta.yml" >"${base_meta_file}" 2>/dev/null; then
    base_meta_file=""
  fi

  while IFS= read -r line; do
    var_name="${line%%:*}"
    old_digest="$(grep -oP '[0-9a-f]{64}' <<<"${line}")"
    recompute_digest "${file}" "${base_file}" "${meta_file}" "${base_meta_file}" \
      "${app}" "${var_name}" "${old_digest}" \
      || status=1
  done < <(grep -E '^[a-z0-9_]+_(sha256|checksum): "(sha256:)?[0-9a-f]{64}"$' "${file}" || true)
  return "${status}"
}

# A version bump now lands in the App's Playbook Meta, so a changed meta
# must re-render the digest-bearing defaults file of the same App.
changed_defaults_files() {
  local base_ref="${1}" file app
  {
    git diff --name-only "${base_ref}...HEAD" -- 'ansible/roles/*/defaults/main.yml'
    while IFS= read -r file; do
      app="$(basename "${file}" .meta.yml)"
      if [[ -f "ansible/roles/${app}/defaults/main.yml" ]]; then
        echo "ansible/roles/${app}/defaults/main.yml"
      fi
    done < <(git diff --name-only "${base_ref}...HEAD" -- 'ansible/playbooks/*.meta.yml')
  } | sort -u
}

main() {
  local base_ref="${1:?usage: recompute-checksums.sh <base-ref>}"
  local status=0 file
  while IFS= read -r file; do
    process_file "${base_ref}" "${file}" || status=1
  done < <(changed_defaults_files "${base_ref}")
  exit "${status}"
}

main "$@"
