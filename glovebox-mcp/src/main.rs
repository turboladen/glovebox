//! The `glovebox-mcp` binary: serves the glovebox tools and resources over
//! stdio. Stdout carries only protocol frames, so every diagnostic goes to
//! stderr.

use glovebox_mcp::GloveboxMcp;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

const DEFAULT_DB_PATH: &str = "data/glovebox.db";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The subscriber is installed before anything else runs so that no log
    // line can fall back to a default writer on stdout.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let db_path = std::env::var("GLOVEBOX_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_owned());
    let db = glovebox_shared::db::connect(&db_path).await?;
    tracing::info!(%db_path, "serving glovebox MCP on stdio");

    let service = GloveboxMcp::new(db).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
