#!/usr/bin/env bash
# Layering gate: glovebox-shared is the transport-agnostic domain, so its
# sources must not import axum or rmcp. glovebox-web renders what the shared
# services return, so its manifest and sources must not name sea-orm.
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
  1) ;;
  *)
    echo "FAIL: grep errored with status $status"
    exit "$status"
    ;;
esac

status=0
grep -rnEi 'sea[-_]orm' glovebox-web/Cargo.toml glovebox-web/src glovebox-web/tests || status=$?

case "$status" in
  0)
    echo "FAIL: glovebox-web names sea-orm (it holds glovebox_shared::db::DatabaseConnection and calls shared services)"
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
