//! Domain input structs (plain data) consumed by service functions.
//! HTTP request DTOs in glovebox-backend map INTO these; the MCP surface builds them directly.

pub mod accident;
pub mod ai_provider;
pub mod build;
pub mod document;
pub mod mileage;
pub mod model_template;
pub mod observation;
pub mod part;
pub mod part_slot;
pub mod platform;
pub mod research;
pub mod schedule;
pub mod service_record;
pub mod shop;
pub mod vehicle;
