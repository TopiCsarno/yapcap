// SPDX-License-Identifier: MPL-2.0

use crate::error::GeminiError;
use crate::providers::gemini::buckets::RetrieveUserQuotaResponse;
use serde::Deserialize;
use serde_json::json;

pub const LOAD_CODE_ASSIST_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
pub const RETRIEVE_USER_QUOTA_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";
pub const CLOUD_RESOURCE_MANAGER_PROJECTS_URL: &str =
    "https://cloudresourcemanager.googleapis.com/v1/projects";
pub const GEN_LANG_CLIENT_PROJECT_PREFIX: &str = "gen-lang-client";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadCodeAssist {
    pub tier_id: Option<String>,
    pub cloudaicompanion_project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTier {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cloudaicompanion_project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLoadCodeAssist {
    #[serde(default, rename = "currentTier")]
    current_tier: Option<RawTier>,
    #[serde(default, rename = "cloudaicompanionProject")]
    cloudaicompanion_project: Option<String>,
}

pub fn parse_load_code_assist(raw: &str) -> Result<LoadCodeAssist, String> {
    let parsed: RawLoadCodeAssist = serde_json::from_str(raw)
        .map_err(|error| format!("failed to decode loadCodeAssist response: {error}"))?;
    let tier_id = parsed
        .current_tier
        .as_ref()
        .and_then(|tier| tier.id.clone());
    let cloudaicompanion_project = parsed
        .cloudaicompanion_project
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            parsed
                .current_tier
                .as_ref()
                .and_then(|tier| tier.cloudaicompanion_project.clone())
                .filter(|value| !value.trim().is_empty())
        });
    Ok(LoadCodeAssist {
        tier_id,
        cloudaicompanion_project,
    })
}

pub async fn load_code_assist(
    client: &reqwest::Client,
    access_token: &str,
) -> Result<LoadCodeAssist, String> {
    load_code_assist_at(client, LOAD_CODE_ASSIST_URL, access_token).await
}

pub async fn load_code_assist_at(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
) -> Result<LoadCodeAssist, String> {
    let body = json!({
        "metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
            "duetProject": "default",
        }
    });
    let response = client
        .post(endpoint)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("loadCodeAssist request failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read loadCodeAssist response: {error}"))?;
    if !status.is_success() {
        let snippet = body.trim().chars().take(256).collect::<String>();
        return Err(format!(
            "loadCodeAssist returned {status} (body: {snippet})"
        ));
    }
    parse_load_code_assist(&body)
}

pub async fn load_code_assist_typed(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
) -> Result<LoadCodeAssist, GeminiError> {
    let body = json!({
        "metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
            "duetProject": "default",
        }
    });
    let response = client
        .post(endpoint)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(GeminiError::LoadCodeAssistRequest)?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GeminiError::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        return Err(GeminiError::RateLimited { retry_after_secs });
    }
    if !status.is_success() {
        return Err(GeminiError::LoadCodeAssistHttp {
            status: status.as_u16(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|error| GeminiError::LoadCodeAssistParse(error.to_string()))?;
    parse_load_code_assist(&body).map_err(GeminiError::LoadCodeAssistParse)
}

pub async fn retrieve_user_quota_typed(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
    project_id: &str,
) -> Result<RetrieveUserQuotaResponse, GeminiError> {
    let body = json!({ "project": project_id });
    let response = client
        .post(endpoint)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(GeminiError::QuotaRequest)?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GeminiError::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        return Err(GeminiError::RateLimited { retry_after_secs });
    }
    if !status.is_success() {
        return Err(GeminiError::QuotaHttp {
            status: status.as_u16(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|error| GeminiError::QuotaParse(error.to_string()))?;
    serde_json::from_str(&body).map_err(|error| GeminiError::QuotaParse(error.to_string()))
}

#[derive(Debug, Deserialize)]
struct RawListProjectsResponse {
    #[serde(default)]
    projects: Vec<RawProject>,
}

#[derive(Debug, Deserialize)]
struct RawProject {
    #[serde(default, rename = "projectId")]
    project_id: Option<String>,
    #[serde(default, rename = "lifecycleState")]
    lifecycle_state: Option<String>,
}

pub fn pick_gen_lang_client_project(raw: &str) -> Result<Option<String>, String> {
    let parsed: RawListProjectsResponse = serde_json::from_str(raw)
        .map_err(|error| format!("failed to decode cloudresourcemanager response: {error}"))?;
    Ok(parsed
        .projects
        .into_iter()
        .filter(|p| {
            p.lifecycle_state
                .as_deref()
                .is_none_or(|state| state == "ACTIVE")
        })
        .filter_map(|p| p.project_id)
        .find(|id| id.starts_with(GEN_LANG_CLIENT_PROJECT_PREFIX)))
}

pub async fn fetch_gen_lang_client_project(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
) -> Result<Option<String>, GeminiError> {
    let response = client
        .get(endpoint)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(GeminiError::LoadCodeAssistRequest)?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GeminiError::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        return Err(GeminiError::RateLimited { retry_after_secs });
    }
    if !status.is_success() {
        return Err(GeminiError::LoadCodeAssistHttp {
            status: status.as_u16(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|error| GeminiError::LoadCodeAssistParse(error.to_string()))?;
    pick_gen_lang_client_project(&body).map_err(GeminiError::LoadCodeAssistParse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let fixture = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/gemini/load_code_assist_response.json"
        ))
        .expect("fixture");
        let captured: serde_json::Value = serde_json::from_str(&fixture).expect("json");
        let body = captured["body_text"].as_str().expect("body_text");
        let parsed = parse_load_code_assist(body).expect("parsed");
        assert_eq!(parsed.tier_id.as_deref(), Some("free-tier"));
        assert_eq!(
            parsed.cloudaicompanion_project.as_deref(),
            Some("example-project")
        );
    }

    #[test]
    fn missing_tier_yields_none() {
        let parsed = parse_load_code_assist("{}").expect("parsed");
        assert!(parsed.tier_id.is_none());
        assert!(parsed.cloudaicompanion_project.is_none());
    }

    #[test]
    fn cloud_resource_manager_picks_first_gen_lang_client_project() {
        let raw = r#"{"projects":[
            {"projectId":"my-other-project","lifecycleState":"ACTIVE"},
            {"projectId":"gen-lang-client-0001234567","lifecycleState":"ACTIVE"},
            {"projectId":"gen-lang-client-0007654321","lifecycleState":"ACTIVE"}
        ]}"#;
        assert_eq!(
            pick_gen_lang_client_project(raw).unwrap().as_deref(),
            Some("gen-lang-client-0001234567")
        );
    }

    #[test]
    fn cloud_resource_manager_skips_deleted_projects() {
        let raw = r#"{"projects":[
            {"projectId":"gen-lang-client-0001234567","lifecycleState":"DELETE_REQUESTED"},
            {"projectId":"gen-lang-client-0007654321","lifecycleState":"ACTIVE"}
        ]}"#;
        assert_eq!(
            pick_gen_lang_client_project(raw).unwrap().as_deref(),
            Some("gen-lang-client-0007654321")
        );
    }

    #[test]
    fn cloud_resource_manager_returns_none_when_no_match() {
        let raw = r#"{"projects":[{"projectId":"unrelated","lifecycleState":"ACTIVE"}]}"#;
        assert!(pick_gen_lang_client_project(raw).unwrap().is_none());
    }

    #[test]
    fn cloud_resource_manager_returns_none_when_empty() {
        assert!(pick_gen_lang_client_project("{}").unwrap().is_none());
    }

    #[test]
    fn falls_back_to_tier_project() {
        let raw = r#"{"currentTier":{"id":"standard-tier","cloudaicompanion_project":"proj-1"}}"#;
        let parsed = parse_load_code_assist(raw).expect("parsed");
        assert_eq!(parsed.tier_id.as_deref(), Some("standard-tier"));
        assert_eq!(parsed.cloudaicompanion_project.as_deref(), Some("proj-1"));
    }
}
