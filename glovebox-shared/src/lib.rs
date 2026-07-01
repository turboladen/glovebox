// Intentional conventions that conflict with clippy::pedantic (see CLAUDE.md):
#![allow(clippy::option_option, clippy::struct_field_names, clippy::wildcard_imports)]

pub mod config;
pub mod entities;
pub mod migration;
pub mod services;
