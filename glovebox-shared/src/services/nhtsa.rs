use serde::{Deserialize, Serialize};

const NHTSA_RECALLS_URL: &str = "https://api.nhtsa.gov/recalls/recallsByVehicle";

/// How long one NHTSA request may take before it counts as failed.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// How long a whole recall lookup may take, across every model name it tries.
const LOOKUP_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

// One client serves every request, so a lookup that tries several model
// names reuses its connection.
static CLIENT: std::sync::LazyLock<Result<reqwest::Client, String>> =
    std::sync::LazyLock::new(|| {
        reqwest::Client::builder()
            .build()
            .map_err(|e| e.to_string())
    });

/// Sends a GET to an NHTSA endpoint, failing once [`REQUEST_TIMEOUT`] passes.
// The callers run inside a tool call and promise that a failed lookup does
// not block the work around it, so a stalled connection must end as an error.
pub(crate) async fn get(url: impl reqwest::IntoUrl) -> Result<reqwest::Response, String> {
    CLIENT
        .as_ref()
        .map_err(Clone::clone)?
        .get(url)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| e.to_string())
}

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
    /// The recall's component, or `Recall <campaign number>` when the component is blank.
    pub title: String,
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

/// Asks NHTSA for the recalls on a make, model and year.
///
/// NHTSA lists recalls by model line, so a model it does not list is asked
/// for again with its last word dropped, until one is listed: "Golf GTI"
/// becomes "Golf". The result's `model` is the name that was listed, and its
/// recalls cover every car of that line.
///
/// # Errors
/// A message for an empty make or model, a model line NHTSA does not list, a
/// failed request, or a lookup that takes longer than 30 seconds in all.
pub async fn check_recalls(
    make: &str,
    model: &str,
    model_year: i32,
) -> Result<RecallCheckResult, String> {
    lookup(NHTSA_RECALLS_URL, LOOKUP_BUDGET, make, model, model_year).await
}

async fn lookup(
    base_url: &str,
    budget: std::time::Duration,
    make: &str,
    model: &str,
    model_year: i32,
) -> Result<RecallCheckResult, String> {
    let words: Vec<&str> = model.split_whitespace().collect();
    if make.trim().is_empty() || words.is_empty() {
        return Err("Make and model are required for recall lookup".to_string());
    }
    let every_name = async {
        // NHTSA answers 400 for a model it does not list.
        for end in (1..=words.len()).rev() {
            let name = words[..end].join(" ");
            match fetch_recalls(base_url, make, &name, model_year).await {
                Ok(result) => return Ok(result),
                Err(NhtsaError::BadRequest(_)) => {
                    tracing::warn!("NHTSA does not list model '{name}' for {make} {model_year}");
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(format!(
            "NHTSA Recalls API does not recognize model '{model}' for {make} {model_year}"
        ))
    };
    tokio::time::timeout(budget, every_name)
        .await
        .unwrap_or_else(|_| {
            Err(format!(
                "NHTSA did not answer within {} seconds",
                budget.as_secs()
            ))
        })
}

#[derive(Debug)]
enum NhtsaError {
    BadRequest(String),
    Network(String),
    Parse(String),
}

impl std::fmt::Display for NhtsaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest(url) => write!(f, "NHTSA returned 400 Bad Request for {url}"),
            Self::Network(msg) => write!(f, "NHTSA request failed: {msg}"),
            Self::Parse(msg) => write!(f, "Failed to parse NHTSA response: {msg}"),
        }
    }
}

async fn fetch_recalls(
    base_url: &str,
    make: &str,
    model: &str,
    model_year: i32,
) -> Result<RecallCheckResult, NhtsaError> {
    let url = reqwest::Url::parse_with_params(
        base_url,
        &[
            ("make", make),
            ("model", model),
            ("modelYear", &model_year.to_string()),
        ],
    )
    .map_err(|e| NhtsaError::Network(format!("Failed to build URL: {e}")))?;

    let resp = get(url.clone()).await.map_err(NhtsaError::Network)?;

    if resp.status() == reqwest::StatusCode::BAD_REQUEST {
        return Err(NhtsaError::BadRequest(url.to_string()));
    }
    if !resp.status().is_success() {
        return Err(NhtsaError::Network(format!(
            "NHTSA returned status {}",
            resp.status()
        )));
    }

    let data: NhtsaRecallsResponse = resp
        .json()
        .await
        .map_err(|e| NhtsaError::Parse(e.to_string()))?;

    let recalls = parse_recall_results(data.results);

    Ok(RecallCheckResult {
        make: make.to_string(),
        model: model.to_string(),
        model_year,
        recall_count: i32::try_from(recalls.len()).unwrap_or(i32::MAX),
        recalls,
    })
}

/// Rewrites an NHTSA component label for reading: title case, with a space
/// after each colon. `FUEL SYSTEM, GASOLINE:DELIVERY:FUEL PUMP` becomes
/// `Fuel System, Gasoline: Delivery: Fuel Pump`. A letter starts a word unless
/// it follows a letter, digit or apostrophe.
pub(crate) fn readable_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 4);
    let mut word_continues = false;
    for ch in raw.trim().chars() {
        if ch == ':' {
            out.push_str(": ");
            word_continues = false;
        } else if ch.is_whitespace() && out.ends_with(' ') {
            // Whitespace after a space collapses into it.
        } else if ch.is_alphanumeric() {
            if word_continues {
                out.extend(ch.to_lowercase());
            } else {
                out.extend(ch.to_uppercase());
            }
            word_continues = true;
        } else {
            out.push(ch);
            word_continues = ch == '\'';
        }
    }
    out.trim_end().to_owned()
}

fn parse_recall_results(results: Vec<NhtsaRecallResult>) -> Vec<RecallInfo> {
    results
        .into_iter()
        .filter_map(|r| {
            let campaign_number = r.nhtsa_campaign_number?;
            let title = r
                .component
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map_or_else(|| format!("Recall {campaign_number}"), readable_component);
            Some(RecallInfo {
                campaign_number,
                manufacturer: r.manufacturer,
                title,
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

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const ONE_RECALL: &str =
        r#"{"results":[{"NHTSACampaignNumber":"24V110000","Component":"FUEL SYSTEM"}]}"#;
    const BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

    // Stands in for NHTSA on a local port. It answers by the `model` in the
    // query: `Golf` has one recall, `Stalls` never answers in time, and any
    // other model is a 400. The returned counter holds the requests seen.
    async fn fake_nhtsa() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/recalls", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(n) if n > 0 => request.extend_from_slice(&chunk[..n]),
                            _ => return,
                        }
                    }
                    let request = String::from_utf8_lossy(&request).into_owned();
                    let line = request.lines().next().unwrap_or_default();
                    let (status, body) = if line.contains("model=Golf&") {
                        ("200 OK", ONE_RECALL)
                    } else if line.contains("model=Stalls&") {
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                        ("200 OK", ONE_RECALL)
                    } else {
                        ("400 Bad Request", "{}")
                    };
                    let reply = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: \
                         {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });
        (base, seen)
    }

    #[tokio::test]
    async fn a_model_nhtsa_does_not_list_falls_back_to_the_model_line() {
        let (base, seen) = fake_nhtsa().await;
        let found = lookup(&base, BUDGET, "Volkswagen", "Golf GTI Autobahn", 2017)
            .await
            .unwrap();
        assert_eq!(found.model, "Golf");
        assert_eq!(found.recalls.len(), 1);
        assert_eq!(found.recalls[0].campaign_number, "24V110000");
        // Three words, three requests: the full name, then one word fewer each time.
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_model_nhtsa_lists_is_asked_for_once() {
        let (base, seen) = fake_nhtsa().await;
        let found = lookup(&base, BUDGET, "Volkswagen", "Golf", 2017)
            .await
            .unwrap();
        assert_eq!(found.model, "Golf");
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_model_line_nhtsa_does_not_list_is_an_error() {
        let (base, _) = fake_nhtsa().await;
        let err = lookup(&base, BUDGET, "Volkswagen", "Jetta GLI", 2017)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            "NHTSA Recalls API does not recognize model 'Jetta GLI' for Volkswagen 2017"
        );
    }

    #[tokio::test]
    async fn a_lookup_that_outlasts_its_budget_is_an_error() {
        let (base, _) = fake_nhtsa().await;
        let budget = std::time::Duration::from_millis(200);
        let started = std::time::Instant::now();
        let err = lookup(&base, budget, "Volkswagen", "Stalls", 2017)
            .await
            .unwrap_err();
        assert!(err.contains("did not answer"), "got: {err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

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
                    "Component": "FUEL SYSTEM, GASOLINE:DELIVERY:FUEL PUMP",
                    "Summary": "Fuel pump may fail causing engine stall.",
                    "Consequence": "Engine stall increases crash risk.",
                    "Remedy": "Dealers will replace the fuel pump.",
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
        assert_eq!(
            recalls[0].title,
            "Fuel System, Gasoline: Delivery: Fuel Pump"
        );
        assert_eq!(
            recalls[0].component.as_deref(),
            Some("FUEL SYSTEM, GASOLINE:DELIVERY:FUEL PUMP")
        );
        assert_eq!(recalls[1].title, "Air Bags");
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
                    "Component": "Test Component",
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
                    "NHTSACampaignNumber": "20V999000"
                }
            ]
        }"#;
        let recalls = parse_response_json(json).unwrap();
        assert_eq!(recalls.len(), 1);
        assert_eq!(recalls[0].campaign_number, "20V999000");
        assert!(recalls[0].manufacturer.is_none());
        assert!(recalls[0].remedy.is_none());
        assert_eq!(recalls[0].title, "Recall 20V999000");
    }

    #[test]
    fn a_component_is_title_cased_with_a_space_after_each_colon() {
        assert_eq!(
            readable_component("FUEL SYSTEM, GASOLINE:DELIVERY:FUEL PUMP"),
            "Fuel System, Gasoline: Delivery: Fuel Pump"
        );
        assert_eq!(
            readable_component("  ELECTRICAL SYSTEM/BATTERY: x "),
            "Electrical System/Battery: X"
        );
        assert_eq!(readable_component("Owner's Manual"), "Owner's Manual");
    }

    #[test]
    fn a_blank_component_falls_back_to_the_campaign_number() {
        let json = r#"{"results":[{"NHTSACampaignNumber":"21V001000","Component":"  "}]}"#;
        let recalls = parse_response_json(json).unwrap();
        assert_eq!(recalls[0].title, "Recall 21V001000");
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
                { "NHTSACampaignNumber": "17V123000", "Component": "Valid" },
                { "NHTSACampaignNumber": null, "Component": "Filtered out" }
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
