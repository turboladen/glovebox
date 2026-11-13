#!/usr/bin/env bash
# Layering gate: glovebox-shared is the transport-agnostic domain, so its
# sources must not import axum or rmcp.
#
# Run from the repo root: ./scripts/check-layering.sh
set -euo pipefail

cd "$(dirname "$0")/.."

status=0
grep -rnwE 'axum|rmcp' glovebox-shared/src || status=$?

case "$status" in
  0)
    echo "FAIL: glovebox-shared/src references axum or rmcp (the domain must stay transport-agnostic)"
    exit 1
    ;;
  1)
    echo "layering OK"
    ;;
  *)
    echo "FAIL: grep errored with status $status"
    exit "$status"
    ;;
esac
