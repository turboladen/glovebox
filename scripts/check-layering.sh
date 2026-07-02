#!/usr/bin/env bash
# Layering enforcement for the glovebox workspace (puby):
#   glovebox-shared  = domain (inputs, validation, SQL, business logic) — HTTP-agnostic
#   glovebox-backend = thin Axum surface (DTO <-> domain-input mapping, DomainError -> ApiError)
#
# Run from the repo root: ./scripts/check-layering.sh
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0

# --- Check 1: no SeaORM persistence/query usage in backend handlers -----------
#
# Deliberately NOT the naive `\.(filter|insert|one|all|update|delete|exec)\(`
# grep from the plan — that false-positives on non-SQL code that legitimately
# lives in the backend:
#   - axum MethodRouter builders in router() fns and main.rs routing, e.g.
#     `.route("/{id}", get(get_one).put(update).delete(delete))`
#   - Iterator::filter closures, e.g. `.filter(|m| !m.id.is_empty())` in
#     api/ai.rs model-list filtering
# Instead we grep for SeaORM-specific tokens that only appear when code talks
# to the database directly. Any hit means SQL leaked out of glovebox-shared.
seaorm_tokens='Entity::(find|insert|update|delete)'
seaorm_tokens+='|ActiveModel|IntoActiveModel'
seaorm_tokens+='|find_by_id|delete_many|update_many|insert_many'
seaorm_tokens+='|\.exec\(|\.begin\('
seaorm_tokens+='|QueryFilter|ColumnTrait|EntityTrait|TransactionTrait|PaginatorTrait|QueryOrder|QuerySelect'
if grep -rnE "$seaorm_tokens" glovebox-backend/src/api; then
  echo "FAIL: SeaORM query/persistence usage found in glovebox-backend/src/api (must live in glovebox-shared/src/services)"
  fail=1
fi

# --- Check 2: no sea_orm imports in backend handlers (except DbErr) -----------
#
# The only sea_orm type the API layer legitimately touches is DbErr, for error
# mapping (api/error.rs `From<sea_orm::DbErr> for ApiError`, api/reminders.rs
# RecordNotFound match) — and those reference it fully qualified today. Any
# other sea_orm import (especially `use sea_orm::*`) is a sign a handler is
# about to run queries.
if grep -rn 'use sea_orm' glovebox-backend/src/api | grep -v 'DbErr'; then
  echo "FAIL: sea_orm import (other than DbErr) found in glovebox-backend/src/api"
  fail=1
fi

# --- Check 3: glovebox-shared must not depend on axum -------------------------
if grep -qE '^\s*axum' glovebox-shared/Cargo.toml; then
  echo "FAIL: glovebox-shared depends on axum (domain must stay HTTP-agnostic)"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi

echo "layering OK"
