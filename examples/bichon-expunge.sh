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

# Print UTC date offset by -$1 days, formatted with $2.
# GNU coreutils (Linux) and BSD (macOS) take incompatible flags; dispatch on
# whichever the host's `date` binary accepts.
date_offset() {
  if date -u -d "-1 day" '+%Y' >/dev/null 2>&1; then
    date -u -d "-$1 days" "$2"
  else
    date -u -v-"$1"d "$2"
  fi
}

CUTOFF_DATE=$(date_offset "${WINDOW_DAYS}" '+%Y-%m-%d')
CUTOFF_YM=$(date_offset "${WINDOW_DAYS}" '+%Y/%m')

echo "==> Checking archive freshness on ${BICHON_HOST}…"
ssh "${BICHON_HOST}" "journalctl -u bichon-archive.service --since '-2h' | tail -5"

echo ""
echo "==> Counting IMAP messages in ${FOLDER} older than ${WINDOW_DAYS} days (before ${CUTOFF_DATE})…"
# Use --output json: the default table format includes header rows and box
# drawing, so wc -l is off by a few. jq 'length' counts envelopes precisely.
IMAP_COUNT=$(himalaya --account "${IMAP_ACCOUNT}" envelope list \
  --folder "${FOLDER}" \
  --query "before:${CUTOFF_DATE}" \
  --page-size 9999 \
  --output json \
  2>/dev/null \
  | jq 'length')
echo "    IMAP messages in window: ${IMAP_COUNT}"

echo ""
echo "==> Counting archived messages for ${FOLDER} on ${BICHON_HOST}…"
ARCHIVE_DIR="${BICHON_ARCHIVE_PATH}/${SAFE_ACCOUNT}"
# Archive path encodes message Date as YYYY/MM; folder identity lives in the
# <id>.meta.json sidecar. The IMAP query is folder-scoped, so the archive
# count must also be — counting account-wide .eml files would over-count and
# pass the coverage gate when the target folder is partially archived.
# shellcheck disable=SC2087  # heredoc expansion is intentional: send vars to remote
EML_COUNT=$(ssh "${BICHON_HOST}" bash <<REMOTE
set -euo pipefail
find "${ARCHIVE_DIR}" -regextype posix-extended \
  -regex '.*/[0-9]{4}/[0-9]{2}/[^/]+\\.meta\\.json' 2>/dev/null \
| while IFS= read -r meta; do
    ym=\$(printf '%s' "\$meta" | awk -F/ '{ print \$(NF-2)"/"\$(NF-1) }')
    [ "\$ym" \> "${CUTOFF_YM}" ] && continue
    jq -e --arg f "${FOLDER}" '.folder == \$f' "\$meta" >/dev/null 2>&1 && printf '.\n'
  done | wc -l
REMOTE
)
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
