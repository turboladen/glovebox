//! MCP (Model Context Protocol) server for glovebox.
//!
//! Exposes the domain in `glovebox-shared` to AI clients as semantic
//! domain-verb tools (`record_service`, `check_due_maintenance`, …) and
//! read-only resources (`glovebox://vehicles/{id}`, …). [`router`] serves
//! them over Streamable HTTP in **stateless mode**: every POST is
//! self-contained, there is no session table, and responses are plain JSON.
//! A server restart therefore never invalidates a connected client's session.
//!
//! # Deployment posture
//!
//! **`/mcp` has no authentication.** The server is single-user and meant for
//! a trusted LAN: anyone who can reach the port can read and write every
//! vehicle record. Do not port-forward or reverse-proxy it to the internet
//! as-is. rmcp's DNS-rebinding defense stays on: the Streamable HTTP
//! transport rejects requests whose `Host` header is not allowlisted
//! (localhost variants by default). A LAN deploy reached by hostname (for
//! example `garage-pi.local:3003`) must opt in through the
//! `GLOVEBOX_MCP_ALLOWED_HOSTS` environment variable: comma-separated, and an
//! entry without a port matches any port.

use std::sync::Arc;

use axum::Router;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager,
    tower::{StreamableHttpServerConfig, StreamableHttpService},
};
use sea_orm::DatabaseConnection;

use self::handler::GloveboxMcp;

mod handler;
mod schemas;

/// Build the Axum router for the MCP endpoint, served by the given database
/// connection.
pub fn router(db: DatabaseConnection) -> Router {
    let default_config = StreamableHttpServerConfig::default();
    let allowed_hosts = merge_allowed_hosts(
        default_config.allowed_hosts.clone(),
        std::env::var("GLOVEBOX_MCP_ALLOWED_HOSTS").ok().as_deref(),
    );
    // Stateless: no sessions to strand on restart (see crate docs), and
    // plain-JSON responses instead of SSE, which is simpler for HTTP-only
    // bridges.
    let mut http_config = default_config.with_allowed_hosts(allowed_hosts);
    http_config.legacy_session_mode = false;
    http_config.json_response = true;

    let streamable = StreamableHttpService::new(
        move || Ok(GloveboxMcp::new(db.clone())),
        // Required by the constructor; unused when legacy_session_mode is false.
        Arc::new(LocalSessionManager::default()),
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
