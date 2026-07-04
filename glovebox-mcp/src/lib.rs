//! MCP (Model Context Protocol) server — glovebox's second surface.
//!
//! Exposes the domain in `glovebox-shared` to AI clients as semantic
//! domain-verb tools (`record_service`, `check_due_maintenance`, …) and
//! read-only resources (`glovebox://vehicles/{id}`, …). Mounted into the
//! backend's Axum router at `/mcp` via [`router`]; the transport is
//! Streamable HTTP (single endpoint, POST for JSON-RPC, GET for SSE).
//!
//! # Deployment posture (read before exposing beyond the LAN)
//!
//! **`/mcp` has no authentication — deliberately.** It matches the rest of
//! the app: the whole HTTP API is unauthenticated, single-user, deployed on
//! a trusted LAN. Anyone who can reach the port can read and write every
//! vehicle record, over `/api` and `/mcp` alike. Two consequences:
//!
//! 1. **LAN scoping is the security boundary.** Do not port-forward or
//!    reverse-proxy this server to the internet as-is. If glovebox ever
//!    needs remote access, add an auth layer first — fewd's per-person
//!    bearer-token middleware (`fewd/server/src/mcp/mod.rs`) is the
//!    reference implementation to copy.
//! 2. **rmcp's DNS-rebinding defense is still on.** The Streamable HTTP
//!    transport rejects requests whose `Host` header isn't allowlisted
//!    (localhost variants by default). A LAN deploy reached by hostname
//!    (e.g. `garage-pi.local:3003`) must opt in via the
//!    `GLOVEBOX_MCP_ALLOWED_HOSTS` env var — comma-separated, an entry
//!    without a port matches any port.

use std::{sync::Arc, time::Duration};

use axum::Router;
use glovebox_shared::config::AppConfig;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager,
    tower::{StreamableHttpServerConfig, StreamableHttpService},
};
use sea_orm::DatabaseConnection;

use self::handler::GloveboxMcp;

mod handler;
mod schemas;

/// Build the Axum router for the MCP endpoint. The backend mounts this at
/// `/mcp` via `.nest_service("/mcp", glovebox_mcp::router(db, config))`.
/// The config supplies `files_dir` for `attach_document`'s file storage.
pub fn router(db: DatabaseConnection, config: Arc<AppConfig>) -> Router {
    // Extend rmcp's idle-session reaper from its 5-minute default to 7 days:
    // long enough that a chat client left idle overnight doesn't come back to
    // a stale session id (a 404 most clients surface badly), short enough
    // that sessions from crashed clients don't accumulate forever.
    let mut session_manager = LocalSessionManager::default();
    session_manager.session_config.keep_alive = Some(Duration::from_hours(24 * 7));

    let default_config = StreamableHttpServerConfig::default();
    let allowed_hosts = merge_allowed_hosts(
        default_config.allowed_hosts.clone(),
        std::env::var("GLOVEBOX_MCP_ALLOWED_HOSTS").ok().as_deref(),
    );
    let http_config = default_config.with_allowed_hosts(allowed_hosts);

    let streamable = StreamableHttpService::new(
        move || Ok(GloveboxMcp::new(db.clone(), config.clone())),
        Arc::new(session_manager),
        http_config,
    );

    Router::new().fallback_service(streamable)
}

/// Build the MCP host allowlist by appending operator-supplied hostnames from
/// `GLOVEBOX_MCP_ALLOWED_HOSTS` to rmcp's localhost defaults (read live from
/// `StreamableHttpServerConfig::default()` so an rmcp upgrade can't silently
/// drift this out of sync). Env-var format: comma-separated hostnames,
/// e.g. `GLOVEBOX_MCP_ALLOWED_HOSTS=garage-pi.local,glovebox.lan:3003`; per
/// rmcp matching rules an entry without a port matches any port.
fn merge_allowed_hosts(defaults: Vec<String>, env_value: Option<&str>) -> Vec<String> {
    let mut hosts = defaults;
    if let Some(raw) = env_value {
        for entry in raw.split(',') {
            let trimmed = entry.trim();
            if !trimmed.is_empty() {
                hosts.push(trimmed.to_string());
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::merge_allowed_hosts;
    use rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig;

    fn fake_defaults() -> Vec<String> {
        vec!["alpha".into(), "beta".into()]
    }

    #[test]
    fn unset_env_keeps_defaults_unchanged() {
        assert_eq!(merge_allowed_hosts(fake_defaults(), None), fake_defaults());
    }

    #[test]
    fn blank_entries_are_skipped_and_hosts_trimmed() {
        let mut expected = fake_defaults();
        expected.push("garage-pi.local".into());
        expected.push("glovebox.lan:3003".into());
        assert_eq!(
            merge_allowed_hosts(
                fake_defaults(),
                Some("  garage-pi.local , , glovebox.lan:3003 ")
            ),
            expected,
        );
    }

    /// Confirms the call site reads rmcp's defaults instead of hardcoding
    /// them — if rmcp ever changes its default allowlist this surfaces it.
    #[test]
    fn merge_preserves_rmcp_defaults_verbatim() {
        let rmcp_defaults = StreamableHttpServerConfig::default().allowed_hosts;
        assert_eq!(
            merge_allowed_hosts(rmcp_defaults.clone(), None),
            rmcp_defaults,
        );
    }
}
