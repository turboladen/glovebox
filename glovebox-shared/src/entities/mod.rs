/// Implements `ActiveModelBehavior` for the entity it is written in so that
/// an insert fills each named stamp the caller left unset with the current
/// time. No stamp column has a default, so an insert that bypasses this (a
/// bulk insert or raw SQL) sets its stamps itself. An update is left alone:
/// the service functions stamp `updated_at`.
macro_rules! stamp_on_insert {
    ($($stamp:ident),+) => {
        #[async_trait::async_trait]
        impl ActiveModelBehavior for ActiveModel {
            async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
            where
                C: ConnectionTrait,
            {
                if insert {
                    let now = crate::services::now();
                    $(
                        if self.$stamp.is_not_set() {
                            self.$stamp = sea_orm::ActiveValue::Set(now);
                        }
                    )+
                }
                Ok(self)
            }
        }
    };
}
use stamp_on_insert;

pub mod appointment;
pub mod incident;
pub mod incident_followup;
pub mod incident_service_link;
pub mod job_need;
pub mod maintenance_schedule_item;
pub mod mileage_log;
pub mod model_template;
pub mod need_option;
pub mod part;
pub mod part_content;
pub mod platform;
pub mod research_finding;
pub mod research_report;
pub mod service_recall_link;
pub mod service_record;
pub mod service_record_line_item;
pub mod service_schedule_link;
pub mod shop;
pub mod todo;
pub mod vehicle;
pub mod vehicle_attribute;
pub mod work_item;
