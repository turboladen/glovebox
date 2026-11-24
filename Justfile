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

# rustfmt.toml uses nightly-only options, so both format recipes run a nightly
# toolchain. CI sets GLOVEBOX_NIGHTLY to the dated nightly it installs.
nightly := env("GLOVEBOX_NIGHTLY", "nightly")

# Format check
fmt-check:
    cargo +{{nightly}} fmt --all --check

# Apply formatting
fmt:
    cargo +{{nightly}} fmt --all

# Layering gate: no axum or rmcp in glovebox-shared
check-layering:
    ./scripts/check-layering.sh

# The backend gates run clippy over every target and then rustdoc first, so a
# lint or a bad doc link fails fast, then build and test. Cargo.toml sets the
# lint levels.
ci-backend:
    cargo clippy --workspace --all-targets --locked -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
    cargo build --workspace --locked
    cargo test --workspace --locked

# This spell-checks the tree in US English (`brew install typos-cli`).
typos:
    typos

# Fail on any RustSec advisory for a locked dependency (`cargo install
# cargo-audit`). `.cargo/audit.toml` lists the advisories that are accepted.
audit:
    cargo audit --deny warnings

# Run all CI gates locally
ci: fmt-check check-layering typos audit ci-backend

# --- Install ---

# Build the release binary and print what to put in Claude Desktop's config
install:
    cargo build --release -p glovebox-mcp
    @echo "Binary:  $(pwd)/target/release/glovebox-mcp"
    @echo "Claude Desktop config: $HOME/Library/Application Support/Claude/claude_desktop_config.json"
