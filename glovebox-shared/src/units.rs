//! Lengths as the shop wrote them, and the conversions that compare them.

use serde::ser::{Serialize, SerializeStruct, Serializer};

pub use crate::entities::wear_reading::LengthUnit;

/// A length in the number and unit it was written in, such as 7/32 in or
/// 4 mm. It compares and subtracts through [`Measure::mm`], so readings in
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

    /// The length in millimeters, the unit every comparison and wear rate
    /// is reckoned in.
    pub fn mm(self) -> f64 {
        self.value * mm_per(self.unit)
    }

    /// `mm` millimeters written in `unit`, rounded to two decimal places of it.
    pub fn from_mm(mm: f64, unit: LengthUnit) -> Self {
        let value = mm / mm_per(unit);
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

// The inch is defined as exactly 25.4 mm. A units library would earn its
// place once glovebox records a second kind of quantity, such as pressure.
const MM_PER_INCH: f64 = 25.4;

const fn mm_per(unit: LengthUnit) -> f64 {
    match unit {
        LengthUnit::Millimeter => 1.0,
        LengthUnit::ThirtySecondInch => MM_PER_INCH / 32.0,
        LengthUnit::Inch => MM_PER_INCH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_in_different_units_compare_as_lengths() {
        let tread = Measure::new(8.0, LengthUnit::ThirtySecondInch);
        let metric = Measure::new(6.35, LengthUnit::Millimeter);
        assert!((tread.mm() - metric.mm()).abs() < 1e-9);
        assert!(
            Measure::new(4.0, LengthUnit::Millimeter).mm()
                > Measure::new(3.0, LengthUnit::ThirtySecondInch).mm()
        );
        assert_eq!(
            Measure::from_mm(tread.mm(), LengthUnit::Inch),
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
