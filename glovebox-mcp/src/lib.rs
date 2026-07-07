//! MCP (Model Context Protocol) server — glovebox's second surface.
//!
//! Exposes the domain in `glovebox-shared` to AI clients as semantic
//! domain-verb tools (`record_service`, `check_due_maintenance`, …) and
//! read-only resources (`glovebox://vehicles/{id}`, …). Mounted into the
//! backend's Axum router at `/mcp` via [`router`]; the transport is
//! Streamable HTTP in **stateless mode** — every POST is self-contained,
//! no session table, plain-JSON responses. Statefulness bought us nothing
//! (we send no server-initiated notifications) and cost us a real failure:
//! a backend restart invalidated every client's `mcp-session-id`, and
//! bridges like `mcp-remote` never re-initialize after the resulting 404s —
//! Claude Desktop just looked "unreachable" until relaunched. The dev loop
//! restarts the backend on every recompile, so restarts must be invisible.
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
//! 3. **Everything under `GLOVEBOX_INBOX_DIR` is exposable to the LAN.**
//!    `attach_document`'s `source_path` lets any LAN peer ingest any file
//!    inside the inbox into the document store and read it back over
//!    `/files` (containment prevents reaching *outside* the inbox, symlinks
//!    included). Point the inbox at a DEDICATED directory used only for
//!    glovebox hand-offs — never a broad directory like a home folder or an
//!    AI client's general working dir.

use std::sync::Arc;

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
/// The config supplies `files_dir` for `attach_document`'s file storage and
/// `inbox_dir` for its `source_path` inbox (where LLM clients drop real
/// files so the bytes never travel through model context).
pub fn router(db: DatabaseConnection, config: Arc<AppConfig>) -> Router {
    let default_config = StreamableHttpServerConfig::default();
    let allowed_hosts = merge_allowed_hosts(
        default_config.allowed_hosts.clone(),
        std::env::var("GLOVEBOX_MCP_ALLOWED_HOSTS").ok().as_deref(),
    );
    // Stateless: no sessions to strand on restart (see crate docs), and
    // plain-JSON responses instead of SSE — simpler for HTTP-only bridges.
    let mut http_config = default_config.with_allowed_hosts(allowed_hosts);
    http_config.stateful_mode = false;
    http_config.json_response = true;

    let streamable = StreamableHttpService::new(
        move || Ok(GloveboxMcp::new(db.clone(), config.clone())),
        // Required by the constructor; unused when stateful_mode is false.
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
