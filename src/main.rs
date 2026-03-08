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
        // Vehicle sub-resources (flat routes for correct path param extraction)
        .route("/api/vehicles/{vehicle_id}/mileage", get(api::mileage::list).post(api::mileage::create))
        .route("/api/vehicles/{vehicle_id}/services", get(api::services::list).post(api::services::create))
        .route("/api/vehicles/{vehicle_id}/services/{id}", get(api::services::get_one).put(api::services::update))
        .route("/api/vehicles/{vehicle_id}/schedule", get(api::schedules::resolve))
        .route("/api/vehicles/{vehicle_id}/reminders", get(api::reminders::get_reminders))
        // Observations (per vehicle)
        .route("/api/vehicles/{vehicle_id}/observations", get(api::observations::list).post(api::observations::create))
        .route("/api/vehicles/{vehicle_id}/observations/{id}", get(api::observations::get_one).put(api::observations::update))
        // Accidents (per vehicle)
        .route("/api/vehicles/{vehicle_id}/accidents", get(api::accidents::list).post(api::accidents::create))
        .route("/api/vehicles/{vehicle_id}/accidents/{id}", get(api::accidents::get_one).put(api::accidents::update))
        .route("/api/vehicles/{vehicle_id}/accidents/{accident_id}/correspondence", get(api::accidents::list_correspondence).post(api::accidents::create_correspondence))
        // VIN decode
        .route("/api/vin/{vin}", get(api::vin::decode))
        .route("/api/vehicles/{vehicle_id}/vin-decode/{vin}", axum::routing::post(api::vin::decode_and_store))
        // Top-level resources
        .nest("/api/vehicles", api::vehicles::router())
        .nest("/api/platforms", api::platforms::router())
        .nest("/api/model-templates", api::model_templates::router())
        .nest("/api/schedules", api::schedules::router())
        .nest("/api/settings", api::settings::router())
        .nest("/api/shops", api::shops::router())
        // Documents (top-level, query-filtered)
        .route("/api/documents", get(api::documents::list).post(api::documents::upload))
        .route("/api/documents/{id}", get(api::documents::get_one).delete(api::documents::delete))
        .nest_service("/files", ServeDir::new(&files_dir))
        .fallback_service(spa_fallback)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("Listening on {}", listen_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
