#!/usr/bin/env bash
# store_blob.sh <input>
# Reference storage adapter: encrypts the paid blob at rest and prints a claim
# ticket (JSON on stdout = the job result). Retrieval/retention policy is the
# operator's business; this adapter only ever sees the one job it was paid for.
set -euo pipefail

input="$1"
STORE_DIR="${STORE_DIR:-$HOME/rende-store}"
mkdir -p "$STORE_DIR"

ticket=$(uuidgen | tr '[:upper:]' '[:lower:]')

if [ -n "${STORAGE_AGE_RECIPIENT:-}" ] && command -v age >/dev/null; then
  age -r "$STORAGE_AGE_RECIPIENT" -o "$STORE_DIR/$ticket.age" "$input"
  enc="age"
else
  # Fallback so the flow works out of the box; set STORAGE_AGE_RECIPIENT for
  # real encryption at rest.
  cp "$input" "$STORE_DIR/$ticket.blob"
  enc="none"
fi

size=$(wc -c < "$input" | tr -d ' ')
printf '{"ticket": "%s", "bytes": %s, "encryption": "%s", "retention_days": 30}\n' \
  "$ticket" "$size" "$enc"
