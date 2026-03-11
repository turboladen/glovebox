// Intentional conventions that conflict with clippy::pedantic:
// - Option<Option<T>> in update DTOs distinguishes "not sent" vs "set to null"
// - Entity field names like vehicle_name on Vehicle map to DB column names
// - sea_orm::* glob imports are idiomatic for SeaORM (prelude-style)
#![allow(
    clippy::option_option,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]

mod api;
mod config;
mod entities;
mod migration;
mod services;

use std::path::Path;
use std::sync::Arc;

use axum::{routing::get, Router};
use clap::Parser;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use config::AppConfig;
use migration::Migrator;
use services::ai::registry::AiProviderRegistry;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<AppConfig>,
    pub ai: Arc<AiProviderRegistry>,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
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

    let ai = Arc::new(AiProviderRegistry::new(db.clone()));

    let has_provider = ai.any_configured().await.unwrap_or(false);
    tracing::info!("AI providers configured: {has_provider}");

    let state = AppState {
        db,
        config: Arc::new(config),
        ai,
    };

    let listen_addr = state.config.listen.clone();
    let files_dir = state.config.files_dir.clone();

    let spa_fallback = ServeDir::new("frontend/dist")
        .not_found_service(ServeFile::new("frontend/dist/index.html"));

    let app = Router::new()
        .route("/api/health", get(api::health::health_check))
        // AI endpoints
        .route("/api/ai/status", get(api::ai::status))
        .route(
            "/api/ai/parse-invoice",
            axum::routing::post(api::ai::parse_invoice),
        )
        .route("/api/ai/chat", axum::routing::post(api::ai::chat))
        .route("/api/ai/models", axum::routing::post(api::ai::fetch_models))
        .route("/api/ai/chat/history", get(api::ai::chat_history))
        .route(
            "/api/ai/providers",
            get(api::ai::list_providers).post(api::ai::create_provider),
        )
        .route(
            "/api/ai/providers/{id}",
            axum::routing::put(api::ai::update_provider).delete(api::ai::delete_provider),
        )
        .route(
            "/api/vehicles/{vehicle_id}/suggestions",
            get(api::ai::get_suggestions),
        )
        // Conversations (per vehicle)
        .route(
            "/api/vehicles/{vehicle_id}/conversations",
            get(api::conversations::list).post(api::conversations::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/conversations/{id}",
            axum::routing::put(api::conversations::rename)
                .delete(api::conversations::delete),
        )
        .route(
            "/api/vehicles/{vehicle_id}/conversations/{id}/messages",
            get(api::conversations::messages),
        )
        // Vehicle sub-resources (flat routes for correct path param extraction)
        .route(
            "/api/vehicles/{vehicle_id}/mileage",
            get(api::mileage::list).post(api::mileage::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/services",
            get(api::services::list).post(api::services::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/services/{id}",
            get(api::services::get_one).put(api::services::update),
        )
        .route(
            "/api/vehicles/{vehicle_id}/schedule",
            get(api::schedules::resolve),
        )
        .route(
            "/api/vehicles/{vehicle_id}/reminders",
            get(api::reminders::get_reminders),
        )
        // Observations (per vehicle)
        .route(
            "/api/vehicles/{vehicle_id}/observations",
            get(api::observations::list).post(api::observations::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/observations/{id}",
            get(api::observations::get_one).put(api::observations::update),
        )
        // Accidents (per vehicle)
        .route(
            "/api/vehicles/{vehicle_id}/accidents",
            get(api::accidents::list).post(api::accidents::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/accidents/{id}",
            get(api::accidents::get_one).put(api::accidents::update),
        )
        .route(
            "/api/vehicles/{vehicle_id}/accidents/{accident_id}/correspondence",
            get(api::accidents::list_correspondence).post(api::accidents::create_correspondence),
        )
        // Part slots and parts (per vehicle)
        .route(
            "/api/vehicles/{vehicle_id}/part-slots",
            get(api::part_slots::list).post(api::part_slots::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/part-slots/{id}",
            get(api::part_slots::get_one)
                .put(api::part_slots::update)
                .delete(api::part_slots::delete),
        )
        .route(
            "/api/vehicles/{vehicle_id}/parts",
            get(api::parts::list).post(api::parts::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/parts/{id}",
            get(api::parts::get_one)
                .put(api::parts::update)
                .delete(api::parts::delete),
        )
        // Research & recalls
        .route(
            "/api/vehicles/{vehicle_id}/recalls",
            get(api::research::check_recalls),
        )
        .route(
            "/api/vehicles/{vehicle_id}/research",
            get(api::research::list_reports).post(api::research::generate_report),
        )
        .route(
            "/api/vehicles/{vehicle_id}/research/findings",
            get(api::research::list_findings),
        )
        .route(
            "/api/vehicles/{vehicle_id}/research/{id}",
            get(api::research::get_report),
        )
        .route(
            "/api/vehicles/{vehicle_id}/research/{report_id}/findings/{id}",
            axum::routing::put(api::research::update_finding_with_body),
        )
        // Cost of ownership
        .route(
            "/api/vehicles/{vehicle_id}/costs",
            get(api::costs::get_costs),
        )
        .route(
            "/api/vehicles/{vehicle_id}/export",
            get(api::export::export_history),
        )
        // VIN decode
        .route("/api/vin/{vin}", get(api::vin::decode))
        .route(
            "/api/vehicles/{vehicle_id}/vin-decode/{vin}",
            axum::routing::post(api::vin::decode_and_store),
        )
        // Top-level resources
        .nest("/api/vehicles", api::vehicles::router())
        .nest("/api/platforms", api::platforms::router())
        .nest("/api/model-templates", api::model_templates::router())
        .nest("/api/schedules", api::schedules::router())
        .nest("/api/shops", api::shops::router())
        // Documents (top-level, query-filtered)
        .route(
            "/api/documents",
            get(api::documents::list).post(api::documents::upload),
        )
        .route(
            "/api/documents/{id}",
            get(api::documents::get_one).delete(api::documents::delete),
        )
        .nest_service("/files", ServeDir::new(&files_dir))
        .fallback_service(spa_fallback)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("Listening on {}", listen_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
