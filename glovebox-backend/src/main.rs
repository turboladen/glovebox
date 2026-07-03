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

use std::{path::Path, sync::Arc};

use axum::{
    Router,
    routing::{get, post},
};
use clap::Parser;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};

use glovebox_shared::{config::AppConfig, migration::Migrator};

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<AppConfig>,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "glovebox_backend=debug,glovebox_shared=debug,tower_http=debug".into()
            }),
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
        // Full-text search (one domain operation over vehicles/events/documents)
        .route("/api/search", get(api::search::search))
        // Garage-wide dashboard + merged activity feeds (unit F)
        .route("/api/dashboard", get(api::dashboard::get_dashboard))
        .route(
            "/api/dashboard/activity",
            get(api::dashboard::garage_activity),
        )
        .route(
            "/api/vehicles/{vehicle_id}/activity",
            get(api::dashboard::vehicle_activity),
        )
        // Planning: work items + visits (unit F HTTP surface over unit G's
        // primitives)
        .route(
            "/api/vehicles/{vehicle_id}/work-items",
            get(api::plan::list_work_items).post(api::plan::create_work_item),
        )
        .route(
            "/api/vehicles/{vehicle_id}/work-items/{id}",
            axum::routing::put(api::plan::update_work_item).delete(api::plan::delete_work_item),
        )
        .route(
            "/api/vehicles/{vehicle_id}/visits",
            get(api::plan::list_visits).post(api::plan::create_visit),
        )
        .route(
            "/api/vehicles/{vehicle_id}/visits/{id}",
            axum::routing::put(api::plan::update_visit).delete(api::plan::delete_visit),
        )
        .route(
            "/api/vehicles/{vehicle_id}/visits/{id}/complete",
            post(api::plan::complete_visit),
        )
        .route(
            "/api/vehicles/{vehicle_id}/visits/{id}/cancel",
            post(api::plan::cancel_visit),
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
            get(api::services::get_one)
                .put(api::services::update)
                .delete(api::services::delete),
        )
        .route(
            "/api/vehicles/{vehicle_id}/schedule",
            get(api::schedules::resolve),
        )
        .route(
            "/api/vehicles/{vehicle_id}/schedule/{item_id}/dismiss",
            post(api::schedules::dismiss).delete(api::schedules::undismiss),
        )
        .route(
            "/api/vehicles/{vehicle_id}/reminders",
            get(api::reminders::get_reminders),
        )
        // 12-month budget forecast (unit G; thin — the Costs UI lands with unit F)
        .route(
            "/api/vehicles/{vehicle_id}/budget",
            get(api::budget::get_budget),
        )
        // Incidents (per vehicle; unified observations + accidents)
        .route(
            "/api/vehicles/{vehicle_id}/incidents",
            get(api::incidents::list).post(api::incidents::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/incidents/{id}",
            get(api::incidents::get_one).put(api::incidents::update),
        )
        .route(
            "/api/vehicles/{vehicle_id}/incidents/{incident_id}/followups",
            get(api::incidents::list_followups).post(api::incidents::create_followup),
        )
        // Parts (per vehicle)
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
        // Builds (per vehicle, one-shot upgrade/restoration targets)
        .route(
            "/api/vehicles/{vehicle_id}/builds",
            get(api::builds::list).post(api::builds::create),
        )
        .route(
            "/api/vehicles/{vehicle_id}/builds/{id}",
            get(api::builds::get_one)
                .put(api::builds::update)
                .delete(api::builds::delete),
        )
        // Research & recalls
        .route(
            "/api/vehicles/{vehicle_id}/recalls",
            get(api::research::check_recalls),
        )
        .route(
            "/api/vehicles/{vehicle_id}/research",
            get(api::research::list_reports),
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
        // MCP server (glovebox-mcp): the LLM-facing surface over the same
        // domain library. Unauthenticated by design, like the rest of the
        // app — LAN-only deployment posture; see glovebox-mcp's crate docs
        // before exposing this port beyond the LAN.
        //
        // Mounted AFTER the CORS layer on purpose: MCP clients aren't
        // browsers, so /mcp gains nothing from CORS — and permissive CORS
        // here would let any web page a LAN user visits drive the tools
        // from their browser. Without CORS headers, cross-origin
        // preflights simply fail.
        .nest_service("/mcp", glovebox_mcp::router(state.db.clone()))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("Listening on {}", listen_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
