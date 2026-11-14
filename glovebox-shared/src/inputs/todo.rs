/// A to-do to add.
#[derive(Default)]
pub struct NewTodo {
    /// What to do; must not be blank.
    pub title: String,
    pub notes: Option<String>,
    /// The appointment the to-do belongs to; it must be the vehicle's.
    pub appointment_id: Option<i32>,
}

/// Changes to a to-do. An absent field is left alone; an explicit `None` in a
/// double option clears the column.
#[derive(Default)]
pub struct UpdateTodo {
    pub title: Option<String>,
    pub notes: Option<Option<String>>,
    pub appointment_id: Option<Option<i32>>,
}
