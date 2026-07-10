# Run Playwright e2e tests (requires `just dev` running in another terminal)
test-e2e:
    cd frontend && bunx playwright test

# Run Playwright e2e tests with visible browser
test-e2e-ui:
    cd frontend && bunx playwright test --headed

# Delete the SQLite database (migrations will recreate it on next run)
reset-db:
    rm -f data/glovebox.db data/glovebox.db-shm data/glovebox.db-wal
    @echo "Database deleted. It will be recreated on next 'cargo run'."

# Run backend and frontend together for development
dev:
    #!/usr/bin/env bash
    set -euo pipefail

    # Install frontend deps if needed
    if [ ! -d frontend/node_modules ]; then
        echo "Installing frontend dependencies..."
        cd frontend && bun install && cd ..
    fi

    # Start backend (public URL points at the Vite origin so MCP deep links
    # resolve through the dev proxy)
    GLOVEBOX_PUBLIC_URL=http://localhost:5373 cargo run -p glovebox-backend &
    BACKEND_PID=$!

    # Start frontend dev server
    cd frontend && bun run dev &
    FRONTEND_PID=$!

    # Cleanup on exit
    trap "kill $BACKEND_PID $FRONTEND_PID 2>/dev/null" EXIT
    wait

# --- CI ---
# These recipes are what GitHub Actions runs, so `just ci` reproduces CI locally.

# Format check: nightly rustfmt (rustfmt.toml uses nightly-only options)
fmt-check:
    cargo +nightly fmt --all --check

# Apply formatting
fmt:
    cargo +nightly fmt --all

# Layering gate: no SQL in glovebox-backend handlers, no axum in glovebox-shared
check-layering:
    ./scripts/check-layering.sh

# Backend gates: build, test, clippy (matches CLAUDE.md's pedantic convention)
ci-backend:
    cargo build --workspace --locked
    cargo test --workspace --locked
    cargo clippy --workspace -- -D clippy::pedantic

# Frontend gates: type-check + production build
ci-frontend:
    #!/usr/bin/env bash
    set -euo pipefail
    cd frontend
    bun install --frozen-lockfile
    bun run check
    bun run build

# Run all CI gates locally (everything CI runs except e2e)
ci: fmt-check check-layering ci-backend ci-frontend

# Full e2e for CI: boots backend + vite against a throwaway DB, waits for both
# ports, runs Playwright (single worker — the suite shares one backend DB), tears
# down. Assumes Playwright browsers are installed (CI caches them; locally run
# `cd frontend && bunx playwright install chromium` once).
test-e2e-ci:
    #!/usr/bin/env bash
    set -euo pipefail
    DB=data/e2e.db
    FILES=data/e2e-files
    backend_port=3003
    frontend_port=5373

    # Free the ports (LISTEN-scoped — deliberately not `pkill -f`).
    for p in $backend_port $frontend_port; do
        pids=$(lsof -ti tcp:$p -sTCP:LISTEN 2>/dev/null || true)
        [ -n "$pids" ] && kill $pids 2>/dev/null || true
    done

    rm -f "$DB" "$DB-shm" "$DB-wal"
    rm -rf "$FILES"; mkdir -p "$FILES"

    cargo build --workspace --locked

    GLOVEBOX_DB_PATH="$DB" GLOVEBOX_FILES_DIR="$FILES" GLOVEBOX_LISTEN=0.0.0.0:$backend_port \
        ./target/debug/glovebox-backend >/tmp/glovebox-e2e-backend.log 2>&1 &
    backend_pid=$!
    ( cd frontend && bun run dev >/tmp/glovebox-e2e-frontend.log 2>&1 ) &
    frontend_pid=$!
    trap 'kill $backend_pid $frontend_pid 2>/dev/null || true' EXIT

    echo "Waiting for backend on :$backend_port ..."
    for _ in $(seq 1 60); do curl -sf "http://localhost:$backend_port/api/health" >/dev/null 2>&1 && break; sleep 1; done
    curl -sf "http://localhost:$backend_port/api/health" >/dev/null 2>&1 || { echo "Backend did not start:"; cat /tmp/glovebox-e2e-backend.log; exit 1; }

    echo "Waiting for frontend on :$frontend_port ..."
    for _ in $(seq 1 60); do curl -sf "http://localhost:$frontend_port/" >/dev/null 2>&1 && break; sleep 1; done
    curl -sf "http://localhost:$frontend_port/" >/dev/null 2>&1 || { echo "Frontend did not start:"; cat /tmp/glovebox-e2e-frontend.log; exit 1; }

    cd frontend && bunx playwright test --workers=1
