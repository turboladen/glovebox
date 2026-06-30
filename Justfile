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

    # Start backend
    cargo run &
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

# Backend gates: build, test, clippy (matches CLAUDE.md's pedantic convention)
ci-backend:
    cargo build --locked
    cargo test --locked
    cargo clippy -- -D clippy::pedantic

# Frontend gates: type-check + production build
ci-frontend:
    #!/usr/bin/env bash
    set -euo pipefail
    cd frontend
    bun install --frozen-lockfile
    bun run check
    bun run build

# Run all CI gates locally (everything CI runs except e2e)
ci: fmt-check ci-backend ci-frontend
