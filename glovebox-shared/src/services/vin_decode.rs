use sea_orm::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    entities::{vehicle, vehicle_attribute},
    error::{DomainError, DomainResult},
};

const NHTSA_API_URL: &str = "https://vpic.nhtsa.dot.gov/api/vehicles/DecodeVinValues";

#[derive(Debug, Deserialize)]
struct NhtsaResponse {
    #[serde(rename = "Results")]
    results: Vec<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct VinDecodeResult {
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim: Option<String>,
    pub body_style: Option<String>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub drivetrain: Option<String>,
    /// All decoded attributes as key-value pairs for storage in `vehicle_attributes`
    pub all_attributes: HashMap<String, String>,
}

fn get_str(map: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn get_int(map: &HashMap<String, serde_json::Value>, key: &str) -> Option<i32> {
    map.get(key).and_then(|v| {
        v.as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
}

/// Decode a VIN using the NHTSA vPIC API.
pub async fn decode_vin(vin: &str) -> Result<VinDecodeResult, String> {
    if vin.len() != 17 || !vin.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Invalid VIN: must be exactly 17 alphanumeric characters".to_string());
    }

    let url = format!("{NHTSA_API_URL}/{vin}?format=json");
    let resp = super::nhtsa::get(&url)
        .await
        .map_err(|e| format!("NHTSA API request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("NHTSA API returned status {}", resp.status()));
    }

    let data: NhtsaResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse NHTSA response: {e}"))?;

    let result = data
        .results
        .into_iter()
        .next()
        .ok_or_else(|| "NHTSA response contained no results".to_string())?;

    // Build engine description from components
    let engine = {
        let displacement = get_str(&result, "DisplacementL");
        let cylinders = get_str(&result, "EngineCylinders");
        let config = get_str(&result, "EngineConfiguration");
        let fuel = get_str(&result, "FuelTypePrimary");
        let turbo = get_str(&result, "Turbo");

        let mut parts = Vec::new();
        if let Some(d) = displacement {
            parts.push(format!("{d}L"));
        }
        if let Some(c) = cylinders {
            if let Some(cfg) = config {
                parts.push(format!("{cfg}-{c}"));
            } else {
                parts.push(format!("{c}-cyl"));
            }
        }
        if turbo.as_deref() == Some("Yes") {
            parts.push("Turbo".to_string());
        }
        if let Some(f) = fuel {
            parts.push(f);
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    };

    // Extract named fields before consuming result
    let year = get_int(&result, "ModelYear");
    let make = get_str(&result, "Make");
    let model = get_str(&result, "Model");
    let trim = get_str(&result, "Trim");
    let body_style = get_str(&result, "BodyClass");
    let transmission = get_str(&result, "TransmissionStyle");
    let drivetrain = get_str(&result, "DriveType");

    // Collect all non-empty attributes (consumes result)
    let all_attributes: HashMap<String, String> = result
        .into_iter()
        .filter_map(|(k, v)| {
            let s = v
                .as_str()
                .map(|s| s.trim().to_string())
                .or_else(|| {
                    if v.is_number() {
                        Some(v.to_string())
                    } else {
                        None
                    }
                })
                .filter(|s| !s.is_empty())?;
            Some((k, s))
        })
        .collect();

    Ok(VinDecodeResult {
        year,
        make,
        model,
        trim,
        body_style,
        engine,
        transmission,
        drivetrain,
        all_attributes,
    })
}

/// Replaces a vehicle's `vin_decode`-sourced attributes with the decoded set.
///
/// # Errors
/// `NotFound` for a missing vehicle.
pub async fn store_attributes<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    decoded: &VinDecodeResult,
) -> DomainResult<()> {
    let txn = super::begin_write(db).await?;
    let db = &txn;
    crate::services::vehicle::require(db, vehicle_id).await?;

    // Delete any existing vin_decode attributes for this vehicle, then re-insert
    vehicle_attribute::Entity::delete_many()
        .filter(vehicle_attribute::Column::VehicleId.eq(vehicle_id))
        .filter(vehicle_attribute::Column::Source.eq("vin_decode"))
        .exec(&txn)
        .await?;

    for (key, value) in &decoded.all_attributes {
        let attr = vehicle_attribute::ActiveModel {
            vehicle_id: Set(vehicle_id),
            key: Set(key.clone()),
            value: Set(value.clone()),
            source: Set(Some("vin_decode".to_string())),
            ..Default::default()
        };
        attr.insert(&txn).await?;
    }

    txn.commit().await?;
    Ok(())
}

/// One stored fact about a vehicle, such as `Plant City`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct VehicleAttribute {
    pub key: String,
    pub value: String,
    /// What recorded the attribute; `vin_decode` for an NHTSA decode.
    pub source: Option<String>,
}

/// A vehicle's stored attributes, sorted by key. The list is empty until a
/// VIN decode has been stored for the vehicle.
///
/// # Errors
/// `NotFound` for a missing vehicle.
pub async fn list_attributes(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
) -> DomainResult<Vec<VehicleAttribute>> {
    super::vehicle::require(db, vehicle_id).await?;
    let rows = vehicle_attribute::Entity::find()
        .filter(vehicle_attribute::Column::VehicleId.eq(vehicle_id))
        .order_by_asc(vehicle_attribute::Column::Key)
        .order_by_asc(vehicle_attribute::Column::Id)
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|a| VehicleAttribute {
            key: a.key,
            value: a.value,
            source: a.source,
        })
        .collect())
}

async fn count_attributes(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<u64> {
    Ok(vehicle_attribute::Entity::find()
        .filter(vehicle_attribute::Column::VehicleId.eq(vehicle_id))
        .count(db)
        .await?)
}

/// What decoding a stored vehicle's VIN did.
#[derive(Debug, Serialize)]
pub struct VehicleDecode {
    /// The vehicle after the decode.
    pub vehicle: vehicle::Model,
    /// The vehicle's fields the decode filled, by their names in `vehicle`.
    pub filled_fields: Vec<&'static str>,
    /// How many attributes the vehicle has stored after the call.
    pub attributes_stored: u64,
    /// Why the lookup failed. It is `None` when the lookup succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vin_decode_error: Option<String>,
}

/// Decodes a stored vehicle's VIN on NHTSA (a live web request), replaces its
/// stored `vin_decode` attributes with the decoded set, and fills each of
/// year, make, model, trim, body style, engine, transmission and drivetrain
/// that is empty on the vehicle. A field that holds a value keeps it.
///
/// A failed lookup is not an error: the result carries `vin_decode_error`,
/// and the vehicle and its stored attributes are unchanged.
///
/// # Errors
/// `NotFound` for a missing vehicle; `BadRequest` for a vehicle with no VIN,
/// or one whose VIN changed while the lookup ran.
pub async fn decode_for_vehicle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
) -> DomainResult<VehicleDecode> {
    decode_for_vehicle_with(db, vehicle_id, |vin| async move { decode_vin(&vin).await }).await
}

fn stored_vin(v: &vehicle::Model) -> Option<&str> {
    v.vin
        .as_deref()
        .map(str::trim)
        .filter(|vin| !vin.is_empty())
}

/// [`decode_for_vehicle`] with the VIN lookup supplied by the caller, so a
/// test can stand in for the network.
pub(crate) async fn decode_for_vehicle_with<C, F, Fut>(
    db: &C,
    vehicle_id: i32,
    lookup: F,
) -> DomainResult<VehicleDecode>
where
    C: ConnectionTrait + TransactionTrait,
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<VinDecodeResult, String>>,
{
    let before = super::vehicle::require(db, vehicle_id).await?;
    let Some(vin) = stored_vin(&before).map(str::to_string) else {
        return Err(DomainError::BadRequest(format!(
            "Vehicle {vehicle_id} has no VIN to decode. Set one with update_vehicle first."
        )));
    };

    // The request runs before the transaction opens, so no write lock is held
    // while NHTSA answers.
    let decoded = match lookup(vin.clone()).await {
        Ok(decoded) => decoded,
        Err(e) => {
            tracing::warn!(vehicle_id, error = %e, "NHTSA VIN decode failed");
            return Ok(VehicleDecode {
                attributes_stored: count_attributes(db, vehicle_id).await?,
                vehicle: before,
                filled_fields: Vec::new(),
                vin_decode_error: Some(e),
            });
        }
    };

    let txn = super::begin_write(db).await?;
    let current = super::vehicle::require(&txn, vehicle_id).await?;
    if stored_vin(&current) != Some(vin.as_str()) {
        return Err(DomainError::BadRequest(format!(
            "Vehicle {vehicle_id}'s VIN changed while it was being decoded. Decode it again."
        )));
    }
    let (vehicle, filled_fields) = fill_empty_fields(&txn, current, &decoded).await?;
    store_attributes(&txn, vehicle_id, &decoded).await?;
    let attributes_stored = count_attributes(&txn, vehicle_id).await?;
    txn.commit().await?;
    Ok(VehicleDecode {
        vehicle,
        filled_fields,
        attributes_stored,
        vin_decode_error: None,
    })
}

/// Sets each decodable field that is empty on the vehicle to the decoded
/// value, and names the fields it set. It runs inside the caller's
/// transaction.
async fn fill_empty_fields(
    db: &impl ConnectionTrait,
    current: vehicle::Model,
    decoded: &VinDecodeResult,
) -> DomainResult<(vehicle::Model, Vec<&'static str>)> {
    let mut filled = Vec::new();
    let mut active = vehicle::ActiveModel {
        id: Unchanged(current.id),
        ..Default::default()
    };
    if let (None, Some(year)) = (current.year, decoded.year) {
        active.year = Set(Some(year));
        filled.push("year");
    }
    let texts = [
        (
            "make",
            &mut active.make,
            current.make.as_ref(),
            decoded.make.as_ref(),
        ),
        (
            "model",
            &mut active.model,
            current.model.as_ref(),
            decoded.model.as_ref(),
        ),
        (
            "trim_level",
            &mut active.trim_level,
            current.trim_level.as_ref(),
            decoded.trim.as_ref(),
        ),
        (
            "body_style",
            &mut active.body_style,
            current.body_style.as_ref(),
            decoded.body_style.as_ref(),
        ),
        (
            "engine",
            &mut active.engine,
            current.engine.as_ref(),
            decoded.engine.as_ref(),
        ),
        (
            "transmission",
            &mut active.transmission,
            current.transmission.as_ref(),
            decoded.transmission.as_ref(),
        ),
        (
            "drivetrain",
            &mut active.drivetrain,
            current.drivetrain.as_ref(),
            decoded.drivetrain.as_ref(),
        ),
    ];
    for (name, slot, stored, found) in texts {
        if let (None, Some(value)) = (stored, found) {
            *slot = Set(Some(value.clone()));
            filled.push(name);
        }
    }
    if filled.is_empty() {
        return Ok((current, filled));
    }
    active.updated_at = Set(super::now());
    Ok((active.update(db).await?, filled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inputs::vehicle::{NewVehicle, UpdateVehicle},
        test_support::{VehicleFixture, test_db},
    };

    fn decoded() -> VinDecodeResult {
        VinDecodeResult {
            year: Some(2017),
            make: Some("VW".into()),
            model: Some("GTI".into()),
            trim: None,
            body_style: None,
            engine: None,
            transmission: None,
            drivetrain: None,
            all_attributes: HashMap::from([("Make".to_string(), "VW".to_string())]),
        }
    }

    #[tokio::test]
    async fn store_attributes_missing_vehicle_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            store_attributes(&db, 999, &decoded()).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Nothing was written for the bogus vehicle id.
        crate::test_support::assert_empty(
            &vehicle_attribute::Entity::find().all(&db).await.unwrap(),
        );
    }

    #[tokio::test]
    async fn store_attributes_round_trips_for_existing_vehicle() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;

        store_attributes(&db, vid, &decoded()).await.unwrap();
        let attrs = vehicle_attribute::Entity::find().all(&db).await.unwrap();
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].vehicle_id, vid);
        assert_eq!(attrs[0].key, "Make");
    }

    const VIN: &str = "3VW447AU9JM000001";

    fn full_decode() -> VinDecodeResult {
        VinDecodeResult {
            year: Some(2018),
            make: Some("VOLKSWAGEN".into()),
            model: Some("Golf GTI".into()),
            trim: Some("SE".into()),
            body_style: Some("Hatchback".into()),
            engine: Some("2.0L 4-cyl Turbo Gasoline".into()),
            transmission: Some("Manual".into()),
            drivetrain: Some("FWD".into()),
            all_attributes: HashMap::from([
                ("PlantCity".to_string(), "PUEBLA".to_string()),
                ("Make".to_string(), "VOLKSWAGEN".to_string()),
                ("Doors".to_string(), "4".to_string()),
            ]),
        }
    }

    async fn vehicle_with(db: &DatabaseConnection, input: NewVehicle) -> vehicle::Model {
        crate::services::vehicle::create(
            db,
            NewVehicle {
                name: "Daily".into(),
                ..input
            },
        )
        .await
        .unwrap()
    }

    fn keys(attributes: &[VehicleAttribute]) -> Vec<&str> {
        attributes.iter().map(|a| a.key.as_str()).collect()
    }

    #[tokio::test]
    async fn list_attributes_sorts_by_key_and_keeps_vehicles_apart() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        let other = VehicleFixture::new().insert_id(&db).await;
        store_attributes(&db, vid, &full_decode()).await.unwrap();
        store_attributes(&db, other, &decoded()).await.unwrap();

        let listed = list_attributes(&db, vid).await.unwrap();
        assert_eq!(keys(&listed), ["Doors", "Make", "PlantCity"]);
        assert_eq!(
            listed[2],
            VehicleAttribute {
                key: "PlantCity".into(),
                value: "PUEBLA".into(),
                source: Some("vin_decode".into()),
            }
        );
        let others = list_attributes(&db, other).await.unwrap();
        assert_eq!(keys(&others), ["Make"]);
        assert_eq!(others[0].value, "VW");
    }

    #[tokio::test]
    async fn list_attributes_is_empty_before_a_decode_and_not_found_for_no_vehicle() {
        let db = test_db().await;
        let vid = VehicleFixture::new().insert_id(&db).await;
        crate::test_support::assert_empty(&list_attributes(&db, vid).await.unwrap());

        let missing = list_attributes(&db, 99_999).await.unwrap_err();
        let plain = crate::services::vehicle::get(&db, 99_999)
            .await
            .unwrap_err();
        assert!(matches!(missing, DomainError::NotFound(_)), "{missing:?}");
        assert_eq!(missing.to_string(), plain.to_string());
    }

    #[tokio::test]
    async fn decoding_fills_only_the_empty_fields_and_stores_the_attributes() {
        let db = test_db().await;
        let v = vehicle_with(
            &db,
            NewVehicle {
                vin: Some(format!(" {VIN} ")),
                make: Some("VW".into()),
                engine: Some("EA888 Gen 3".into()),
                ..Default::default()
            },
        )
        .await;

        let out = decode_for_vehicle_with(&db, v.id, |vin| async move {
            assert_eq!(vin, VIN, "the lookup gets the VIN without its spaces");
            Ok(full_decode())
        })
        .await
        .unwrap();

        assert_eq!(
            out.filled_fields,
            [
                "year",
                "model",
                "trim_level",
                "body_style",
                "transmission",
                "drivetrain"
            ]
        );
        assert_eq!(out.attributes_stored, 3);
        assert_eq!(out.vin_decode_error, None);
        assert_eq!(out.vehicle.make.as_deref(), Some("VW"));
        assert_eq!(out.vehicle.engine.as_deref(), Some("EA888 Gen 3"));
        assert_eq!(out.vehicle.year, Some(2018));
        assert_eq!(out.vehicle.model.as_deref(), Some("Golf GTI"));
        assert_eq!(out.vehicle.trim_level.as_deref(), Some("SE"));
        assert_eq!(out.vehicle.name, "Daily");
        let stored = crate::services::vehicle::get(&db, v.id).await.unwrap();
        assert_eq!(stored, out.vehicle);
        assert_eq!(list_attributes(&db, v.id).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn decoding_again_replaces_the_attributes_and_fills_nothing() {
        let db = test_db().await;
        let v = vehicle_with(
            &db,
            NewVehicle {
                vin: Some(VIN.into()),
                ..Default::default()
            },
        )
        .await;
        decode_for_vehicle_with(&db, v.id, |_| async { Ok(full_decode()) })
            .await
            .unwrap();
        let first = crate::services::vehicle::get(&db, v.id).await.unwrap();

        // The second answer differs in every field and lists one attribute.
        let out = decode_for_vehicle_with(&db, v.id, |_| async { Ok(decoded()) })
            .await
            .unwrap();

        crate::test_support::assert_empty(&out.filled_fields);
        assert_eq!(out.vehicle, first, "a filled field keeps its value");
        assert_eq!(out.attributes_stored, 1);
        let listed = list_attributes(&db, v.id).await.unwrap();
        assert_eq!(keys(&listed), ["Make"]);
        assert_eq!(listed[0].value, "VW");
    }

    #[tokio::test]
    async fn a_failed_lookup_reports_the_error_and_changes_nothing() {
        let db = test_db().await;
        let v = vehicle_with(
            &db,
            NewVehicle {
                vin: Some(VIN.into()),
                ..Default::default()
            },
        )
        .await;
        store_attributes(&db, v.id, &decoded()).await.unwrap();

        let out = decode_for_vehicle_with(&db, v.id, |_| async {
            Err("NHTSA API request failed: offline".to_string())
        })
        .await
        .unwrap();

        assert_eq!(
            out.vin_decode_error.as_deref(),
            Some("NHTSA API request failed: offline")
        );
        crate::test_support::assert_empty(&out.filled_fields);
        assert_eq!(out.attributes_stored, 1, "the stored decode stays");
        assert_eq!(out.vehicle, v);
        assert_eq!(crate::services::vehicle::get(&db, v.id).await.unwrap(), v);
    }

    #[tokio::test]
    async fn a_vehicle_with_no_vin_is_refused_before_any_lookup() {
        let db = test_db().await;
        for vin in [None, Some("   ".to_string())] {
            let v = vehicle_with(
                &db,
                NewVehicle {
                    vin,
                    ..Default::default()
                },
            )
            .await;
            let err = decode_for_vehicle_with(&db, v.id, |_| async {
                panic!("a vehicle with no VIN must not reach the lookup")
            })
            .await
            .unwrap_err();
            assert!(
                matches!(&err, DomainError::BadRequest(m) if m.contains("update_vehicle")),
                "{err:?}"
            );
        }

        let missing = decode_for_vehicle_with(&db, 99_999, |_| async {
            panic!("a missing vehicle must not reach the lookup")
        })
        .await
        .unwrap_err();
        let plain = crate::services::vehicle::get(&db, 99_999)
            .await
            .unwrap_err();
        assert_eq!(missing.to_string(), plain.to_string());
    }

    #[tokio::test]
    async fn a_vin_changed_during_the_lookup_is_refused_and_writes_nothing() {
        let db = test_db().await;
        let v = vehicle_with(
            &db,
            NewVehicle {
                vin: Some(VIN.into()),
                ..Default::default()
            },
        )
        .await;

        let writer = db.clone();
        let err = decode_for_vehicle_with(&db, v.id, |_| async move {
            crate::services::vehicle::update(
                &writer,
                v.id,
                UpdateVehicle {
                    vin: Some(Some("1HGCM82633A004352".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            Ok(full_decode())
        })
        .await
        .unwrap_err();

        assert!(
            matches!(&err, DomainError::BadRequest(m) if m.contains("VIN changed")),
            "{err:?}"
        );
        let stored = crate::services::vehicle::get(&db, v.id).await.unwrap();
        assert_eq!(stored.year, None);
        crate::test_support::assert_empty(&list_attributes(&db, v.id).await.unwrap());
    }
}
