#!/usr/bin/env bash
# Layering gate: glovebox-shared is the transport-agnostic domain, so its
# sources must not import axum or rmcp. glovebox-web renders what the shared
# services return, so its manifest and sources must not name sea-orm. A
# service transaction opens with services::begin_write (BEGIN IMMEDIATE), so
# glovebox-shared/src/services must not begin one any other way.
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
opened=$(grep -rnE '\.begin\(\)|\.begin_with_(config|options)\(|\.transaction\(' \
  glovebox-shared/src/services) || status=$?

case "$status" in
  0 | 1) ;;
  *)
    echo "FAIL: grep errored with status $status"
    exit "$status"
    ;;
esac
# The one call allowed is the one inside services::begin_write.
opened=$(printf '%s\n' "$opened" | grep -v '^glovebox-shared/src/services/mod.rs:.*begin_with_options' | grep -v '^$' || true)
if [ -n "$opened" ]; then
  printf '%s\n' "$opened"
  echo "FAIL: a service opens a transaction without services::begin_write"
  exit 1
fi

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
