pub mod activity;
pub mod budget;
pub mod build;
pub mod costs;
pub mod dashboard;
pub mod document;
pub mod export;
pub mod incident;
pub mod mileage;
pub mod model_template;
pub mod nhtsa;
pub mod part;
pub mod platform;
pub mod reminders;
pub mod research;
pub mod schedule;
pub mod search;
pub mod service_record;
pub mod shop;
pub mod vehicle;
pub mod vin_decode;
pub mod visit;
pub mod work_item;

/// The project-wide stored-timestamp stamp (naive UTC, stored as text).
pub(crate) fn now_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
