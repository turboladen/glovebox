// Intentional conventions that conflict with clippy::pedantic (see CLAUDE.md):
#![allow(
    clippy::option_option,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
// These pedantic lints target a published API. The only callers of this
// library are the workspace's own programs, so a `# Errors` section or a
// `#[must_use]` on every service function would add text and no information.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::implicit_hasher
)]

pub mod backup;
pub mod dates;
pub mod db;
pub mod entities;
pub mod env;
pub mod error;
pub mod inputs;
pub mod migration;
pub mod money;
pub mod seed;
pub mod services;
pub mod vocab;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
