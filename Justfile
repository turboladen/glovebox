# Copy the database to data/backups/ now. The server also does this itself
# once a day and before every migration, keeping the newest 14.
backup:
    cargo run --quiet --release -p glovebox-cli -- backup

# Delete the named non-live environment's database files. `live` is refused.
reset-env env:
    cargo run --quiet --release -p glovebox-cli -- reset --env {{quote(env)}}

# Fill the named non-live environment with made-up demo data. `live` is refused.
seed env:
    cargo run --quiet --release -p glovebox-cli -- seed --env {{quote(env)}}

# Serve the web page on 127.0.0.1 and open it in the browser. Use `just web test`
# for the made-up data from `just seed test`.
web env="live":
    GLOVEBOX_ENV={{quote(env)}} cargo run --quiet --release -p glovebox-web -- --open

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

# The backend gates run clippy over every target first, so a lint fails fast,
# then build and test. Cargo.toml sets the pedantic level.
ci-backend:
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo build --workspace --locked
    cargo test --workspace --locked

# This spell-checks the tree in US English (`brew install typos-cli`).
typos:
    typos

# Run all CI gates locally
ci: fmt-check check-layering typos ci-backend


# --- Install ---

# Build the release binary and print what to put in Claude Desktop's config
install:
    cargo build --release -p glovebox-mcp
    @echo "Binary:  $(pwd)/target/release/glovebox-mcp"
    @echo "Claude Desktop config: $HOME/Library/Application Support/Claude/claude_desktop_config.json"
