use serde::{Deserialize, Serialize};

const NHTSA_RECALLS_URL: &str = "https://api.nhtsa.gov/recalls/recallsByVehicle";

#[derive(Debug, Deserialize)]
struct NhtsaRecallsResponse {
    results: Vec<NhtsaRecallResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NhtsaRecallResult {
    #[serde(rename = "NHTSACampaignNumber")]
    nhtsa_campaign_number: Option<String>,
    manufacturer: Option<String>,
    subject: Option<String>,
    summary: Option<String>,
    consequence: Option<String>,
    remedy: Option<String>,
    report_received_date: Option<String>,
    component: Option<String>,
    #[serde(rename = "NHTSAActionNumber")]
    nhtsa_action_number: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct RecallInfo {
    pub campaign_number: String,
    pub manufacturer: Option<String>,
    pub subject: String,
    pub summary: Option<String>,
    pub consequence: Option<String>,
    pub remedy: Option<String>,
    pub report_date: Option<String>,
    pub component: Option<String>,
    pub action_number: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct RecallCheckResult {
    pub make: String,
    pub model: String,
    pub model_year: i32,
    pub recall_count: i32,
    pub recalls: Vec<RecallInfo>,
}

pub async fn check_recalls(
    make: &str,
    model: &str,
    model_year: i32,
) -> Result<RecallCheckResult, String> {
    if make.is_empty() || model.is_empty() {
        return Err("Make and model are required for recall lookup".to_string());
    }

    let url = reqwest::Url::parse_with_params(
        NHTSA_RECALLS_URL,
        &[
            ("make", make),
            ("model", model),
            ("modelYear", &model_year.to_string()),
        ],
    )
    .map_err(|e| format!("Failed to build NHTSA URL: {e}"))?;

    let resp = reqwest::get(url)
        .await
        .map_err(|e| format!("NHTSA Recalls API request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "NHTSA Recalls API returned status {}",
            resp.status()
        ));
    }

    let data: NhtsaRecallsResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse NHTSA recalls response: {e}"))?;

    let recalls = parse_recall_results(data.results);

    Ok(RecallCheckResult {
        make: make.to_string(),
        model: model.to_string(),
        model_year,
        recall_count: i32::try_from(recalls.len()).unwrap_or(i32::MAX),
        recalls,
    })
}

fn parse_recall_results(results: Vec<NhtsaRecallResult>) -> Vec<RecallInfo> {
    results
        .into_iter()
        .filter_map(|r| {
            Some(RecallInfo {
                campaign_number: r.nhtsa_campaign_number?,
                manufacturer: r.manufacturer,
                subject: r.subject.unwrap_or_default(),
                summary: r.summary,
                consequence: r.consequence,
                remedy: r.remedy,
                report_date: r.report_received_date,
                component: r.component,
                action_number: r.nhtsa_action_number,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_response_json(json: &str) -> Result<Vec<RecallInfo>, serde_json::Error> {
        let data: NhtsaRecallsResponse = serde_json::from_str(json)?;
        Ok(parse_recall_results(data.results))
    }

    #[test]
    fn parse_empty_recalls_response() {
        let json = r#"{"Count":0,"Message":"Results returned successfully","results":[]}"#;
        let recalls = parse_response_json(json).unwrap();
        assert!(recalls.is_empty());
    }

    #[test]
    fn parse_recalls_with_results() {
        let json = r#"{
            "Count": 2,
            "Message": "Results returned successfully",
            "results": [
                {
                    "Manufacturer": "Volkswagen Group of America, Inc.",
                    "NHTSACampaignNumber": "17V123000",
                    "NHTSAActionNumber": "N/A",
                    "ReportReceivedDate": "03/15/2017",
                    "Component": "FUEL SYSTEM, GASOLINE",
                    "Summary": "Fuel pump may fail causing engine stall.",
                    "Consequence": "Engine stall increases crash risk.",
                    "Remedy": "Dealers will replace the fuel pump.",
                    "Subject": "Fuel Pump Failure",
                    "ModelYear": "2017",
                    "Make": "VOLKSWAGEN",
                    "Model": "GOLF GTI",
                    "Notes": ""
                },
                {
                    "Manufacturer": "Volkswagen Group of America, Inc.",
                    "NHTSACampaignNumber": "18V456000",
                    "NHTSAActionNumber": "",
                    "ReportReceivedDate": "07/20/2018",
                    "Component": "AIR BAGS",
                    "Summary": "Airbag control module software issue.",
                    "Consequence": "Airbags may not deploy.",
                    "Remedy": "Dealers will update the software.",
                    "Subject": "Airbag Control Module",
                    "ModelYear": "2017",
                    "Make": "VOLKSWAGEN",
                    "Model": "GOLF GTI",
                    "Notes": ""
                }
            ]
        }"#;
        let recalls = parse_response_json(json).unwrap();
        assert_eq!(recalls.len(), 2);
        assert_eq!(recalls[0].campaign_number, "17V123000");
        assert_eq!(recalls[0].subject, "Fuel Pump Failure");
        assert_eq!(
            recalls[0].component.as_deref(),
            Some("FUEL SYSTEM, GASOLINE")
        );
        assert_eq!(recalls[1].campaign_number, "18V456000");
    }

    #[test]
    fn parse_recall_missing_campaign_number_is_filtered() {
        let json = r#"{
            "Count": 1,
            "Message": "Results returned successfully",
            "results": [
                {
                    "Manufacturer": "Test",
                    "NHTSACampaignNumber": null,
                    "Subject": "Test Subject",
                    "Summary": "Test summary"
                }
            ]
        }"#;
        let recalls = parse_response_json(json).unwrap();
        assert!(
            recalls.is_empty(),
            "Recalls without campaign number should be filtered out"
        );
    }

    #[test]
    fn parse_recall_missing_optional_fields() {
        let json = r#"{
            "Count": 1,
            "Message": "Results returned successfully",
            "results": [
                {
                    "NHTSACampaignNumber": "20V999000",
                    "Subject": "Minimal Recall"
                }
            ]
        }"#;
        let recalls = parse_response_json(json).unwrap();
        assert_eq!(recalls.len(), 1);
        assert_eq!(recalls[0].campaign_number, "20V999000");
        assert!(recalls[0].manufacturer.is_none());
        assert!(recalls[0].remedy.is_none());
    }

    #[test]
    fn parse_invalid_json() {
        let result = parse_response_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn recall_count_matches_filtered_results() {
        let json = r#"{
            "Count": 2,
            "Message": "Results returned successfully",
            "results": [
                { "NHTSACampaignNumber": "17V123000", "Subject": "Valid" },
                { "NHTSACampaignNumber": null, "Subject": "Filtered out" }
            ]
        }"#;
        let recalls = parse_response_json(json).unwrap();
        assert_eq!(
            recalls.len(),
            1,
            "Only valid recalls should remain after filtering"
        );
    }

    #[test]
    fn check_recalls_rejects_empty_make() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_recalls("", "GTI", 2017));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Make and model are required"));
    }

    #[test]
    fn check_recalls_rejects_empty_model() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_recalls("Volkswagen", "", 2017));
        assert!(result.is_err());
    }
}
