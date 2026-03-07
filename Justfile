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
