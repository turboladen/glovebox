//! MCP (Model Context Protocol) server for glovebox.
//!
//! Exposes the domain in `glovebox-shared` to AI clients as semantic
//! domain-verb tools (`record_service`, `check_due_maintenance`, …) and
//! read-only resources (`glovebox://vehicles/{id}`, …). The `glovebox-mcp`
//! binary serves [`GloveboxMcp`] over stdio, which is how Claude Desktop
//! launches it.
//!
//! # Deployment posture
//!
//! The server has no network listener and no authentication. Whoever can
//! start the process can read and write every vehicle record, so access
//! control is the operating system's file permissions on the database.

mod handler;
mod schemas;

pub use handler::GloveboxMcp;
