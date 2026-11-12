# Delete the SQLite database (migrations will recreate it on next run)
reset-db:
    rm -f data/glovebox.db data/glovebox.db-shm data/glovebox.db-wal
    @echo "Database deleted. It will be recreated on next 'cargo run'."

# --- CI ---
# These recipes are what GitHub Actions runs, so `just ci` reproduces CI locally.

# Format check: nightly rustfmt (rustfmt.toml uses nightly-only options)
fmt-check:
    cargo +nightly fmt --all --check

# Apply formatting
fmt:
    cargo +nightly fmt --all

# Layering gate: no axum or rmcp in glovebox-shared
check-layering:
    ./scripts/check-layering.sh

# Backend gates: build, test, clippy (matches CLAUDE.md's pedantic convention)
ci-backend:
    cargo build --workspace --locked
    cargo test --workspace --locked
    cargo clippy --workspace -- -D clippy::pedantic

# Run all CI gates locally
ci: fmt-check check-layering ci-backend

