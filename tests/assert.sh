#!/usr/bin/env bash
#
# tests/assert.sh
#
# Assertion helpers for the shell test scripts in this directory. Sourced, never
# run. Deliberately not a test framework: the scripts under test are shell, and a
# framework would be a build-time dependency for four asserts.
#
# shellcheck shell=bash

TESTS_RUN=0
TESTS_FAILED=0

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  TESTS_RUN=$((TESTS_RUN + 1))
  if [[ "${actual}" == "${expected}" ]]; then
    printf 'ok   %s\n' "${label}"
    return 0
  fi
  printf 'FAIL %s\n       expected: %s\n       actual:   %s\n' \
    "${label}" "${expected}" "${actual}" >&2
  TESTS_FAILED=$((TESTS_FAILED + 1))
}

assert_succeeds() {
  local label="$1"
  shift
  TESTS_RUN=$((TESTS_RUN + 1))
  if "$@"; then
    printf 'ok   %s\n' "${label}"
    return 0
  fi
  printf 'FAIL %s\n       expected exit 0 from: %s\n' "${label}" "$*" >&2
  TESTS_FAILED=$((TESTS_FAILED + 1))
}

assert_fails() {
  local label="$1"
  shift
  TESTS_RUN=$((TESTS_RUN + 1))
  if ! "$@"; then
    printf 'ok   %s\n' "${label}"
    return 0
  fi
  printf 'FAIL %s\n       expected non-zero from: %s\n' "${label}" "$*" >&2
  TESTS_FAILED=$((TESTS_FAILED + 1))
}

report() {
  local suite="$1"
  printf '\n%s: %s assertions, %s failed\n' "${suite}" "${TESTS_RUN}" "${TESTS_FAILED}" >&2
  ((TESTS_FAILED == 0))
}
