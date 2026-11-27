//! Lengths as the shop wrote them, and the conversions that compare them.

use serde::ser::{Serialize, SerializeStruct, Serializer};
use uom::si::{
    f64::Length,
    length::{inch, millimeter},
};

pub use crate::entities::wear_reading::LengthUnit;

/// A length in the number and unit it was written in, such as 7/32 in or
/// 4 mm. It compares and subtracts through [`Measure::length`], so readings in
/// different units agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub value: f64,
    pub unit: LengthUnit,
}

impl Measure {
    pub const fn new(value: f64, unit: LengthUnit) -> Self {
        Self { value, unit }
    }

    /// The length this measure stands for.
    pub fn length(self) -> Length {
        match self.unit {
            LengthUnit::Millimeter => Length::new::<millimeter>(self.value),
            LengthUnit::ThirtySecondInch => Length::new::<inch>(self.value / 32.0),
            LengthUnit::Inch => Length::new::<inch>(self.value),
        }
    }

    /// `length` written in `unit`, rounded to two decimal places of it.
    pub fn of_length(length: Length, unit: LengthUnit) -> Self {
        let value = match unit {
            LengthUnit::Millimeter => length.get::<millimeter>(),
            LengthUnit::ThirtySecondInch => length.get::<inch>() * 32.0,
            LengthUnit::Inch => length.get::<inch>(),
        };
        Self::new((value * 100.0).round() / 100.0, unit)
    }

    /// The measure as a person reads it: `4 mm`, `7/32 in` or `0.25 in`.
    pub fn text(self) -> String {
        match self.unit {
            LengthUnit::Millimeter => format!("{} mm", self.value),
            LengthUnit::ThirtySecondInch => format!("{}/32 in", self.value),
            LengthUnit::Inch => format!("{} in", self.value),
        }
    }
}

/// Serializes as `value`, `unit` and the `text` a person reads.
impl Serialize for Measure {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("Measure", 3)?;
        out.serialize_field("value", &self.value)?;
        out.serialize_field("unit", &self.unit)?;
        out.serialize_field("text", &self.text())?;
        out.end()
    }
}

/// A length in millimeters, the unit wear rates are reckoned in.
pub fn millimeters(length: Length) -> f64 {
    length.get::<millimeter>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_in_different_units_compare_as_lengths() {
        let tread = Measure::new(8.0, LengthUnit::ThirtySecondInch);
        let metric = Measure::new(6.35, LengthUnit::Millimeter);
        assert!((millimeters(tread.length()) - millimeters(metric.length())).abs() < 1e-9);
        assert!(
            Measure::new(4.0, LengthUnit::Millimeter).length()
                > Measure::new(3.0, LengthUnit::ThirtySecondInch).length()
        );
        assert_eq!(
            Measure::of_length(tread.length(), LengthUnit::Inch),
            Measure::new(0.25, LengthUnit::Inch)
        );
    }

    #[test]
    fn text_reads_as_the_shop_wrote_it() {
        assert_eq!(Measure::new(4.0, LengthUnit::Millimeter).text(), "4 mm");
        assert_eq!(Measure::new(6.5, LengthUnit::Millimeter).text(), "6.5 mm");
        assert_eq!(
            Measure::new(7.0, LengthUnit::ThirtySecondInch).text(),
            "7/32 in"
        );
        assert_eq!(
            serde_json::to_value(Measure::new(3.0, LengthUnit::ThirtySecondInch)).unwrap(),
            serde_json::json!({"value": 3.0, "unit": "in_32", "text": "3/32 in"})
        );
    }
}
