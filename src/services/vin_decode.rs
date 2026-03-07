use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// All decoded attributes as key-value pairs for storage in vehicle_attributes
    pub all_attributes: HashMap<String, String>,
}

/// Decode a VIN using the NHTSA vPIC API.
pub async fn decode_vin(vin: &str) -> Result<VinDecodeResult, String> {
    if vin.len() != 17 || !vin.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Invalid VIN: must be exactly 17 alphanumeric characters".to_string());
    }

    let url = format!("{NHTSA_API_URL}/{vin}?format=json");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("NHTSA API request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("NHTSA API returned status {}", resp.status()));
    }

    let data: NhtsaResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse NHTSA response: {e}"))?;

    let result = data.results.into_iter().next()
        .ok_or_else(|| "NHTSA response contained no results".to_string())?;

    fn get_str(map: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
        map.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn get_int(map: &HashMap<String, serde_json::Value>, key: &str) -> Option<i32> {
        map.get(key)
            .and_then(|v| {
                v.as_i64().map(|n| n as i32)
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
    }

    // Build engine description from components
    let engine = {
        let displacement = get_str(&result, "DisplacementL");
        let cylinders = get_str(&result, "EngineCylinders");
        let config = get_str(&result, "EngineConfiguration");
        let fuel = get_str(&result, "FuelTypePrimary");
        let turbo = get_str(&result, "Turbo");

        let mut parts = Vec::new();
        if let Some(d) = displacement { parts.push(format!("{d}L")); }
        if let Some(c) = cylinders {
            if let Some(cfg) = config {
                parts.push(format!("{cfg}-{c}"));
            } else {
                parts.push(format!("{c}-cyl"));
            }
        }
        if turbo.as_deref() == Some("Yes") { parts.push("Turbo".to_string()); }
        if let Some(f) = fuel { parts.push(f); }

        if parts.is_empty() { None } else { Some(parts.join(" ")) }
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
            let s = v.as_str()
                .map(|s| s.trim().to_string())
                .or_else(|| {
                    if v.is_number() { Some(v.to_string()) } else { None }
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
