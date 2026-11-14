#!/usr/bin/env bash
# Layering gate: glovebox-shared is the transport-agnostic domain, so its
# sources must not import axum or rmcp. glovebox-web renders what the shared
# services return, so its sources must not run a SeaORM query.
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
grep -rnE 'sea_orm::(Entity|ActiveModel|Query|Condition)|EntityTrait|ActiveModelTrait|\.filter\(.*Column::' glovebox-web/src || status=$?

case "$status" in
  0)
    echo "FAIL: glovebox-web/src runs a SeaORM query (queries belong in glovebox-shared services)"
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
