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
