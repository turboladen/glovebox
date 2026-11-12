#!/usr/bin/env bash
# Layering gate: glovebox-shared is the transport-agnostic domain, so its
# sources must not import axum or rmcp.
#
# Run from the repo root: ./scripts/check-layering.sh
set -euo pipefail

cd "$(dirname "$0")/.."

if rg -n '\b(axum|rmcp)\b' glovebox-shared/src; then
  echo "FAIL: glovebox-shared/src references axum or rmcp (the domain must stay transport-agnostic)"
  exit 1
fi

echo "layering OK"
