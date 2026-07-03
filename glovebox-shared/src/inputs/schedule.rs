pub struct NewScheduleItem {
    pub platform_id: Option<i32>,
    pub model_template_id: Option<i32>,
    pub vehicle_id: Option<i32>,
    pub overrides_item_id: Option<i32>,
    pub name: String,
    pub description: Option<String>,
    pub interval_miles: Option<i32>,
    pub interval_months: Option<i32>,
    pub warning_miles: Option<i32>,
    pub warning_days: Option<i32>,
    pub enabled: Option<bool>,
    pub source: Option<String>,
    pub notes: Option<String>,
    pub is_factory_recommended: Option<bool>,
    pub labor_categories: Option<String>,
    /// Estimated cost per occurrence, integer cents (decision ⑧ — feeds
    /// the budget forecast).
    pub est_cost_cents: Option<i32>,
}

#[derive(Default)]
pub struct UpdateScheduleItem {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub interval_miles: Option<Option<i32>>,
    pub interval_months: Option<Option<i32>>,
    pub warning_miles: Option<Option<i32>>,
    pub warning_days: Option<Option<i32>>,
    pub enabled: Option<bool>,
    pub source: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub is_factory_recommended: Option<Option<bool>>,
    pub labor_categories: Option<Option<String>>,
    pub est_cost_cents: Option<Option<i32>>,
}

/// Filter for listing raw schedule items by owner.
#[derive(Default)]
pub struct ScheduleFilter {
    pub platform_id: Option<i32>,
    pub model_template_id: Option<i32>,
    pub vehicle_id: Option<i32>,
}
