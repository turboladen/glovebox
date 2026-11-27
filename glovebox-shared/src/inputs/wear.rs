use chrono::NaiveDate;

use crate::{
    entities::wear_reading::{Axle, WearComponent},
    error::{DomainError, DomainResult},
    units::Measure,
};

/// What a new part replaces, so its install starts a new wear series. A
/// brake part is fitted to one axle; the tires are one set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WearTag {
    BrakePads(Axle),
    BrakeRotors(Axle),
    Tires,
}

impl WearTag {
    /// The tag a component and an axle name.
    ///
    /// # Errors
    /// `Invalid` on `field` for a brake component with no axle, and for the
    /// tires with one.
    pub fn new(field: &str, component: WearComponent, axle: Option<Axle>) -> DomainResult<Self> {
        match (component, axle) {
            (WearComponent::BrakePads, Some(axle)) => Ok(Self::BrakePads(axle)),
            (WearComponent::BrakeRotors, Some(axle)) => Ok(Self::BrakeRotors(axle)),
            (WearComponent::TireTread, None) => Ok(Self::Tires),
            (WearComponent::TireTread, Some(_)) => Err(DomainError::invalid(
                field,
                "the tires are tracked as one set, so a tire part names no axle",
            )),
            (_, None) => Err(DomainError::invalid(
                field,
                "a brake part names the axle it is fitted to",
            )),
        }
    }

    /// The tag a part's stored columns hold, if any. A pair the database's
    /// check would refuse reads as none.
    pub fn of_columns(component: Option<WearComponent>, axle: Option<Axle>) -> Option<Self> {
        component.and_then(|c| Self::new("wears_as", c, axle).ok())
    }

    pub const fn component(self) -> WearComponent {
        match self {
            Self::BrakePads(_) => WearComponent::BrakePads,
            Self::BrakeRotors(_) => WearComponent::BrakeRotors,
            Self::Tires => WearComponent::TireTread,
        }
    }

    pub const fn axle(self) -> Option<Axle> {
        match self {
            Self::BrakePads(axle) | Self::BrakeRotors(axle) => Some(axle),
            Self::Tires => None,
        }
    }
}

/// A wear reading to record. A reading taken during a service names it, and
/// takes that service's date and odometer for each one it leaves out.
#[derive(Clone, Debug)]
pub struct NewWearReading {
    pub component: WearComponent,
    pub axle: Axle,
    pub reading: Measure,
    /// The minimum the shop stated beside the reading.
    pub limit: Option<Measure>,
    pub measured_on: Option<NaiveDate>,
    pub odometer: Option<i32>,
    pub service_id: Option<i32>,
    pub notes: Option<String>,
}

/// A correction to a wear reading: an absent field keeps the stored value,
/// and `Some(None)` clears it.
#[derive(Clone, Debug, Default)]
pub struct UpdateWearReading {
    pub component: Option<WearComponent>,
    pub axle: Option<Axle>,
    pub reading: Option<Measure>,
    pub limit: Option<Option<Measure>>,
    pub measured_on: Option<NaiveDate>,
    pub odometer: Option<Option<i32>>,
    pub service_id: Option<Option<i32>>,
    pub notes: Option<Option<String>>,
}

/// Filter for listing a vehicle's wear readings.
#[derive(Clone, Debug, Default)]
pub struct WearFilter {
    pub component: Option<WearComponent>,
    pub axle: Option<Axle>,
    pub service_id: Option<i32>,
    /// The most readings returned, counted from the newest.
    pub limit: Option<u32>,
}
