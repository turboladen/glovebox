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
    if std::env::args().nth(1).as_deref() == Some("backup") {
        // The messages go to stderr like every other diagnostic, so stdout
        // stays reserved for protocol frames in every mode.
        let shown = std::path::absolute(&db_path)
            .map_or_else(|_| db_path.clone(), |p| p.display().to_string());
        let Some(copy) = glovebox_shared::db::backup_now(&db_path).await? else {
            return Err(format!("nothing to back up: {shown} is missing or empty").into());
        };
        eprintln!("Backed up {shown} to {}", copy.display());
        return Ok(());
    }
    let db = glovebox_shared::db::connect(&db_path).await?;
    tracing::info!(%db_path, "serving glovebox MCP on stdio");

    glovebox_shared::backup::spawn_daily(db.clone(), std::path::PathBuf::from(&db_path));
    let service = GloveboxMcp::new(db).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
