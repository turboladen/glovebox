use serde::{Deserialize, Deserializer};

/// Deserializes `Option<Option<T>>` correctly for update DTOs:
/// - JSON field **absent** → `None` (don't update the field)
/// - JSON field present with `null` → `Some(None)` (set to null)
/// - JSON field present with value → `Some(Some(value))` (set to value)
///
/// Usage: `#[serde(default, deserialize_with = "deserialize_optional")]`
pub fn deserialize_optional<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}
