mod api;
mod config;
mod entities;
mod migration;
mod services;

use std::path::Path;
use std::sync::Arc;

use axum::{Router, routing::get};
use clap::Parser;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use config::AppConfig;
use migration::Migrator;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<AppConfig>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "glovebox=debug,tower_http=debug".into()),
        )
        .init();

    let config = AppConfig::parse();

    // Ensure directories exist
    if let Some(parent) = Path::new(&config.db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&config.files_dir)?;

    // Connect to SQLite with pool-level pragmas via SqliteConnectOptions.
    // SQLx defaults already set: foreign_keys=ON, busy_timeout=5s.
    // We add journal_mode=WAL for concurrent read performance.
    let db_url = format!("sqlite://{}?mode=rwc", &config.db_path);
    let mut opt = ConnectOptions::new(&db_url);
    opt.sqlx_logging(false);
    opt.map_sqlx_sqlite_opts(|sqlite_opts| {
        sqlite_opts.journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
    });
    let db = Database::connect(opt).await?;

    Migrator::up(&db, None).await?;

    tracing::info!("Database ready at {}", &config.db_path);

    let state = AppState {
        db,
        config: Arc::new(config),
    };

    let listen_addr = state.config.listen.clone();
    let files_dir = state.config.files_dir.clone();

    let spa_fallback = ServeDir::new("frontend/dist")
        .not_found_service(ServeFile::new("frontend/dist/index.html"));

    let app = Router::new()
        .route("/api/health", get(api::health::health_check))
        .nest_service("/files", ServeDir::new(&files_dir))
        .fallback_service(spa_fallback)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("Listening on {}", listen_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
