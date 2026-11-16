//! Domain input structs (plain data) consumed by service functions.
//! Callers build most of them directly. `checklist::scope`, `need::part_link`
//! and `need::arrival` build the checklist scope, part link and arrival
//! change from optional arguments and refuse a combination that names no
//! valid one.

pub mod appointment;
pub mod checklist;
pub mod incident;
pub mod mileage;
pub mod model_template;
pub mod need;
pub mod part;
pub mod platform;
pub mod schedule;
pub mod service_record;
pub mod shop;
pub mod todo;
pub mod vehicle;
pub mod work_item;
