//! Domain input structs (plain data) consumed by service functions.
//! Callers build most of them directly.
//! [`services::checklist::scope`](crate::services::checklist::scope),
//! [`services::need::part_link`](crate::services::need::part_link) and
//! [`services::need::arrival`](crate::services::need::arrival) build the
//! checklist scope, part link and arrival change from optional arguments and
//! refuse a combination that names no valid one.

pub mod appointment;
pub mod checklist;
pub mod incident;
pub mod mileage;
pub mod need;
pub mod part;
pub mod schedule;
pub mod service_record;
pub mod todo;
pub mod vehicle;
pub mod work_item;
