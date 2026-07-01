// Intentional conventions that conflict with clippy::pedantic (see CLAUDE.md):
#![allow(clippy::option_option, clippy::struct_field_names, clippy::wildcard_imports)]
// These pedantic lints target public-API surface. They did not fire while this
// code lived in the `glovebox` binary crate (nothing was externally reachable);
// they surface now only because the domain became a library. Phase A is a
// behavior-preserving mechanical move, so we preserve the prior (green) state
// rather than churn every service fn with doc/must_use annotations.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::implicit_hasher
)]

pub mod config;
pub mod entities;
pub mod migration;
pub mod services;
