//! The `glovebox-mcp` binary: serves the glovebox tools and resources over
//! stdio. Stdout carries only protocol frames, so every diagnostic goes to
//! stderr.

use glovebox_mcp::GloveboxMcp;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The subscriber is installed before anything else runs so that no log
    // line can fall back to a default writer on stdout.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let environment = glovebox_shared::env::resolve(|k| std::env::var(k).ok())?;
    let db_path = environment.db_path;
    let db =
        glovebox_shared::db::connect_as(&db_path, glovebox_shared::db::Program::McpServer).await?;
    tracing::info!(env = %environment.name, %db_path, "serving glovebox MCP on stdio");

    glovebox_shared::backup::spawn_daily(db.clone(), std::path::PathBuf::from(&db_path));
    let service = GloveboxMcp::new(db).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
