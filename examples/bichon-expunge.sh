#!/usr/bin/env bash
# examples/bichon-expunge.sh
#
# REFERENCE SCRIPT — NOT A SUPPORTED TOOL
# ----------------------------------------
# This is a worked example for operators who want to safely expunge old mail
# from their Upstream Mailbox after confirming it is present in the Email
# Archive and the off-host backup.  It intentionally stops short of the actual
# expunge; the destructive step is left as an explicit operator command.
#
# Designed for MXroute but should work with any IMAP provider that supports
# himalaya.  Adjust IMAP_HOST, FOLDER, and WINDOW_DAYS for your setup.
#
# Prerequisites:
#   - himalaya  (https://github.com/pimalaya/himalaya — Rust, matches project ethos)
#   - ssh access to the Bichon host
#   - journalctl access on the Bichon host
#
# Usage:
#   BICHON_HOST=yourserver BICHON_ARCHIVE_PATH=/var/lib/bichon-archive \
#   IMAP_ACCOUNT=you@example.com IMAP_HOST=mail.mxrouting.net \
#   FOLDER=INBOX WINDOW_DAYS=90 \
#   bash examples/bichon-expunge.sh
#
# shellcheck shell=bash

set -euo pipefail

: "${BICHON_HOST:?Set BICHON_HOST to the SSH hostname of the Bichon server}"
: "${BICHON_ARCHIVE_PATH:=/var/lib/bichon-archive}"
: "${IMAP_ACCOUNT:?Set IMAP_ACCOUNT to the email address configured in Bichon}"
: "${IMAP_HOST:?Set IMAP_HOST to the IMAP server hostname}"
: "${FOLDER:=INBOX}"
: "${WINDOW_DAYS:=90}"

SAFE_ACCOUNT=$(printf '%s' "$IMAP_ACCOUNT" | tr '/' '_')
CUTOFF_DATE=$(date -u -d "-${WINDOW_DAYS} days" '+%Y-%m-%d')
CUTOFF_YM=$(date -u -d "-${WINDOW_DAYS} days" '+%Y/%m')

echo "==> Checking archive freshness on ${BICHON_HOST}…"
ssh "${BICHON_HOST}" "journalctl -u bichon-archive.service --since '-2h' | tail -5"

echo ""
echo "==> Counting IMAP messages in ${FOLDER} older than ${WINDOW_DAYS} days (before ${CUTOFF_DATE})…"
# himalaya search with 'before:' returns only envelopes older than the cutoff.
IMAP_COUNT=$(himalaya --account "${IMAP_ACCOUNT}" envelope list \
  --folder "${FOLDER}" \
  --query "before:${CUTOFF_DATE}" \
  --page-size 9999 \
  2>/dev/null \
  | wc -l)
echo "    IMAP messages in window: ${IMAP_COUNT}"

echo ""
echo "==> Counting .eml files on ${BICHON_HOST} for the same window…"
ARCHIVE_DIR="${BICHON_ARCHIVE_PATH}/${SAFE_ACCOUNT}"
# Archive path encodes the message Date as YYYY/MM (bichon-archive.sh derives
# it from envelope.date). File mtime reflects archiver write-time, not message
# age, so an -mtime filter would mis-count after backfills and rebuilds.
# shellcheck disable=SC2029  # intentional: expand ARCHIVE_DIR locally before SSH
EML_COUNT=$(ssh "${BICHON_HOST}" \
  "find '${ARCHIVE_DIR}' -regextype posix-extended -regex '.*/[0-9]{4}/[0-9]{2}/[^/]+\\.eml' 2>/dev/null" \
  | awk -F/ -v c="${CUTOFF_YM}" '{ ym=$(NF-2)"/"$(NF-1); if (ym <= c) n++ } END { print n+0 }')
echo "    Archive .eml files in window: ${EML_COUNT}"

echo ""
if [ "${IMAP_COUNT}" -eq 0 ]; then
  echo "ERROR: No IMAP messages found — check FOLDER and IMAP_ACCOUNT." >&2
  exit 1
fi

if [ "${EML_COUNT}" -lt "${IMAP_COUNT}" ]; then
  echo "COVERAGE GAP: archive has ${EML_COUNT} files but IMAP has ${IMAP_COUNT} messages." >&2
  echo "Do NOT expunge until the gap is resolved.  Check:" >&2
  echo "  ssh ${BICHON_HOST} journalctl -u bichon-archive.service" >&2
  echo "  Bichon UI → Accounts → ${IMAP_ACCOUNT} → sync_folders (verify ${FOLDER} is ticked)" >&2
  exit 1
fi

echo "Coverage OK (${EML_COUNT} archive files >= ${IMAP_COUNT} IMAP messages)."
echo ""
echo "==> Pre-conditions satisfied.  The expunge step is intentionally NOT automated."
echo "    To expunge messages older than ${WINDOW_DAYS} days from ${FOLDER}, run:"
echo ""
echo "    himalaya --account ${IMAP_ACCOUNT} message expunge \\"
echo "      --folder '${FOLDER}' --query 'before:${CUTOFF_DATE}'"
echo ""
echo "    Confirm the archive is in an off-host backup before proceeding:"
echo "    auberge backup create --apps bichon   # on your operator laptop"
