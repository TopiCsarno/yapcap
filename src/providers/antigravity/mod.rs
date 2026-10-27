// SPDX-License-Identifier: MPL-2.0

mod account;
mod login;
pub mod plan_label;
pub mod quota;

use crate::account_storage::{ProviderAccountStorage, ProviderAccountTokens};
use crate::error::{AntigravityError, GeminiError, Result};
use crate::model::{ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot, UsageWindow};
use crate::providers::gemini::code_assist;
use crate::providers::google_oauth::{self, GoogleOAuthConfig, GoogleRefreshError};
use chrono::Utc;
use std::borrow::Cow;
use std::path::PathBuf;
use tracing::warn;

pub use account::{apply_login_account, sync_managed_accounts};
#[cfg(test)]
pub use login::AntigravityLoginSuccess;
pub use login::{
    AntigravityLoginEvent, AntigravityLoginState, AntigravityLoginStatus, prepare,
    prepare_for_reauth,
};

pub const DEFAULT_HOST: &str = "cloudcode-pa.googleapis.com";
pub const DEFAULT_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const DEFAULT_CLIENT_SECRET: &str = "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";

pub const SCOPE: &str = "openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/aicode";

pub const IDE_TYPE: &str = "ANTIGRAVITY";
pub const USER_AGENT: &str = "antigravity/cli/1.4.7";

const HOST_ENV: &str = "YAPCAP_ANTIGRAVITY_HOST";
const CLIENT_ID_ENV: &str = "YAPCAP_ANTIGRAVITY_CLIENT_ID";
const CLIENT_SECRET_ENV: &str = "YAPCAP_ANTIGRAVITY_CLIENT_SECRET";

fn env_override(key: &str, default: &'static str) -> Cow<'static, str> {
    match std::env::var(key) {
        Ok(value) if !value.trim().is_empty() => Cow::Owned(value),
        _ => Cow::Borrowed(default),
    }
}

#[must_use]
pub fn host() -> Cow<'static, str> {
    env_override(HOST_ENV, DEFAULT_HOST)
}

#[must_use]
pub fn oauth_config() -> GoogleOAuthConfig {
    GoogleOAuthConfig {
        client_id: env_override(CLIENT_ID_ENV, DEFAULT_CLIENT_ID),
        client_secret: env_override(CLIENT_SECRET_ENV, DEFAULT_CLIENT_SECRET),
        scope: Cow::Borrowed(SCOPE),
    }
}

#[must_use]
pub fn load_code_assist_url() -> String {
    format!("https://{}/v1internal:loadCodeAssist", host())
}

#[must_use]
pub fn retrieve_user_quota_summary_url() -> String {
    format!("https://{}/v1internal:retrieveUserQuotaSummary", host())
}

struct FetchEndpoints {
    load_code_assist: String,
    retrieve_user_quota: String,
    token: String,
}

impl Default for FetchEndpoints {
    fn default() -> Self {
        Self {
            load_code_assist: load_code_assist_url(),
            retrieve_user_quota: retrieve_user_quota_summary_url(),
            token: google_oauth::TOKEN_ENDPOINT.to_string(),
        }
    }
}

pub async fn fetch(
    client: &reqwest::Client,
    account_id: &str,
    account_dir: PathBuf,
) -> Result<UsageSnapshot, AntigravityError> {
    fetch_at(client, account_id, account_dir, FetchEndpoints::default()).await
}

async fn fetch_at(
    client: &reqwest::Client,
    account_id: &str,
    account_dir: PathBuf,
    endpoints: FetchEndpoints,
) -> Result<UsageSnapshot, AntigravityError> {
    let root = account_dir
        .parent()
        .ok_or_else(|| AntigravityError::AccountStorage("invalid account dir path".to_string()))?;
    let storage = ProviderAccountStorage::new(root);

    let now = Utc::now();
    let mut tokens = storage
        .load_tokens(account_id)
        .map_err(|e| AntigravityError::AccountStorage(e.to_string()))?;

    if google_oauth::needs_refresh(tokens.expires_at, now) {
        tokens = reactive_refresh(client, &endpoints.token, &storage, account_id, &tokens).await?;
    }

    let mut refreshed_once = false;
    let load =
        match load_code_assist(client, &endpoints.load_code_assist, &tokens.access_token).await {
            Ok(load) => load,
            Err(AntigravityError::Unauthorized) => {
                tokens = reactive_refresh(client, &endpoints.token, &storage, account_id, &tokens)
                    .await?;
                refreshed_once = true;
                load_code_assist(client, &endpoints.load_code_assist, &tokens.access_token).await?
            }
            Err(other) => return Err(other),
        };
    let tier_id = load.tier_id.clone().unwrap_or_default();
    let project = load.cloudaicompanion_project.clone();

    let summary = match quota::retrieve_user_quota_summary_typed(
        client,
        &endpoints.retrieve_user_quota,
        &tokens.access_token,
        project.as_deref(),
    )
    .await
    {
        Ok(summary) => summary,
        Err(AntigravityError::Unauthorized) if !refreshed_once => {
            tokens =
                reactive_refresh(client, &endpoints.token, &storage, account_id, &tokens).await?;
            quota::retrieve_user_quota_summary_typed(
                client,
                &endpoints.retrieve_user_quota,
                &tokens.access_token,
                project.as_deref(),
            )
            .await?
        }
        Err(other) => return Err(other),
    };

    let windows = quota::normalize_quota_summary(&summary);
    if windows.is_empty() {
        return Err(AntigravityError::NoUsageData);
    }
    let headline = headline_index(&windows);

    let now = Utc::now();
    let metadata = storage
        .load_metadata(account_id)
        .map_err(|e| AntigravityError::AccountStorage(e.to_string()))?;
    let email = Some(metadata.email.clone()).filter(|email| !email.is_empty());
    let plan = plan_label::plan_label(&tier_id).to_string();

    let mut updated_metadata = metadata.clone();
    updated_metadata.antigravity_last_tier_id = load.tier_id.clone();
    updated_metadata.updated_at = now;
    if let Err(error) = storage.save_metadata(account_id, &updated_metadata) {
        warn!(account_id, error = %error, "antigravity metadata save failed");
    }

    let snapshot = UsageSnapshot {
        provider: ProviderId::Antigravity,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline(headline),
        windows,
        provider_cost: None,
        extra_usage: None,
        identity: ProviderIdentity {
            email,
            plan: Some(plan),
            ..Default::default()
        },
    };
    let _ = storage.save_snapshot(account_id, &snapshot);
    Ok(snapshot)
}

fn headline_index(windows: &[UsageWindow]) -> usize {
    let position = |seconds: i64| {
        windows
            .iter()
            .position(|window| window.window_seconds == Some(seconds))
    };
    position(quota::FIVE_HOUR_WINDOW_SECONDS)
        .or_else(|| position(quota::WEEKLY_WINDOW_SECONDS))
        .unwrap_or(0)
}

async fn load_code_assist(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
) -> Result<code_assist::LoadCodeAssist, AntigravityError> {
    code_assist::load_code_assist_typed(client, endpoint, IDE_TYPE, access_token, Some(USER_AGENT))
        .await
        .map_err(map_load_error)
}

fn map_load_error(error: GeminiError) -> AntigravityError {
    match error {
        GeminiError::Unauthorized => AntigravityError::Unauthorized,
        GeminiError::RateLimited { retry_after_secs } => {
            AntigravityError::RateLimited { retry_after_secs }
        }
        GeminiError::LoadCodeAssistRequest(source) => {
            AntigravityError::LoadCodeAssistRequest(source)
        }
        GeminiError::LoadCodeAssistHttp { status } => {
            AntigravityError::LoadCodeAssistHttp { status }
        }
        GeminiError::LoadCodeAssistParse(detail) => AntigravityError::LoadCodeAssistParse(detail),
        other => AntigravityError::LoadCodeAssistParse(other.to_string()),
    }
}

async fn reactive_refresh(
    client: &reqwest::Client,
    token_endpoint: &str,
    storage: &ProviderAccountStorage,
    account_id: &str,
    previous: &ProviderAccountTokens,
) -> Result<ProviderAccountTokens, AntigravityError> {
    let refreshed = google_oauth::refresh_access_token_at(
        &oauth_config(),
        client,
        token_endpoint,
        &previous.refresh_token,
        Utc::now(),
    )
    .await
    .map_err(map_refresh_error)?;
    let new_tokens = ProviderAccountTokens {
        access_token: refreshed.access_token,
        refresh_token: refreshed.refresh_token,
        expires_at: refreshed.expires_at,
        scope: refreshed.scope,
        token_id: None,
    };
    let _ = storage.save_tokens(account_id, &new_tokens);
    Ok(new_tokens)
}

fn map_refresh_error(error: GoogleRefreshError) -> AntigravityError {
    match error {
        GoogleRefreshError::Request(source) => AntigravityError::TokenRefreshRequest(source),
        GoogleRefreshError::RateLimited { retry_after_secs } => {
            AntigravityError::RateLimited { retry_after_secs }
        }
        GoogleRefreshError::Http { status } => AntigravityError::TokenRefreshHttp { status },
        GoogleRefreshError::Decode(source) => AntigravityError::TokenRefreshDecode(source),
        GoogleRefreshError::Parse(detail) => AntigravityError::TokenRefreshParse(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::providers::interface::ProviderAccountHandle;
    use chrono::{Duration, Utc};
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockResponse {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: String,
    }

    async fn server(
        responses: Vec<MockResponse>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = vec![0; 8192];
                let bytes = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
                assert!(request.starts_with(&format!(
                    "{} {} HTTP/1.1\r\n",
                    response.method, response.path
                )));
                let raw = format!(
                    "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status,
                    response.body.len(),
                    response.body
                );
                stream.write_all(raw.as_bytes()).await.unwrap();
                requests.push(request);
            }
            requests
        });
        (format!("http://{addr}"), handle)
    }

    fn endpoints(base: &str) -> FetchEndpoints {
        FetchEndpoints {
            load_code_assist: format!("{base}/load"),
            retrieve_user_quota: format!("{base}/quota"),
            token: format!("{base}/token"),
        }
    }

    fn free_quota_body() -> String {
        let weekly = (Utc::now() + Duration::days(6)).to_rfc3339();
        format!(
            r#"{{"groups":[
                {{"displayName":"Gemini Models","buckets":[
                    {{"displayName":"Weekly Limit","window":"weekly","remainingFraction":0.947,"resetTime":"{weekly}"}}
                ]}},
                {{"displayName":"Claude and GPT models","buckets":[
                    {{"displayName":"Weekly Limit","window":"weekly","remainingFraction":0.938,"resetTime":"{weekly}"}}
                ]}}
            ]}}"#
        )
    }

    fn quota_body() -> String {
        let weekly = (Utc::now() + Duration::days(6)).to_rfc3339();
        let five = (Utc::now() + Duration::hours(4)).to_rfc3339();
        format!(
            r#"{{"groups":[
                {{"displayName":"Gemini Models","buckets":[
                    {{"displayName":"Weekly Limit","window":"weekly","remainingFraction":0.99,"resetTime":"{weekly}"}},
                    {{"displayName":"Five Hour Limit","window":"5h","remainingFraction":0.5,"resetTime":"{five}"}}
                ]}},
                {{"displayName":"Claude and GPT models","buckets":[
                    {{"displayName":"Weekly Limit","window":"weekly","remainingFraction":1,"resetTime":"{weekly}"}},
                    {{"displayName":"Five Hour Limit","window":"5h","remainingFraction":1,"resetTime":"{five}"}}
                ]}}
            ]}}"#
        )
    }

    fn create_account(
        storage: &ProviderAccountStorage,
        expires_at: chrono::DateTime<Utc>,
    ) -> (String, PathBuf) {
        let stored = storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Antigravity,
                email: "user@example.com".to_string(),
                provider_account_id: Some("sub-1".to_string()),
                organization_id: None,
                organization_name: None,
                tokens: ProviderAccountTokens {
                    access_token: "old-access".to_string(),
                    refresh_token: "old-refresh".to_string(),
                    expires_at,
                    scope: vec!["openid".to_string()],
                    token_id: None,
                },
                snapshot: None,
            })
            .unwrap();
        (stored.account_ref.account_id, stored.account_dir)
    }

    #[tokio::test]
    async fn fetch_builds_four_grouped_windows_with_five_hour_headline() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"free-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: quota_body(),
            },
        ])
        .await;

        let snapshot = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap();

        assert_eq!(snapshot.provider, ProviderId::Antigravity);
        assert_eq!(snapshot.windows.len(), 4);
        assert_eq!(snapshot.identity.plan.as_deref(), Some("Free"));
        assert_eq!(snapshot.identity.email.as_deref(), Some("user@example.com"));
        assert_eq!(snapshot.headline, UsageHeadline(0));
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("\"ideType\":\"ANTIGRAVITY\""));
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("user-agent: antigravity")
        );
        let meta = storage.load_metadata(&account_id).unwrap();
        assert_eq!(meta.antigravity_last_tier_id.as_deref(), Some("free-tier"));
    }

    #[tokio::test]
    async fn fetch_detects_pro_plan_from_paid_tier_and_sends_cli_user_agent() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"free-tier"},"paidTier":{"id":"g1-pro-tier"},"cloudaicompanionProject":"superb-project"}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: quota_body(),
            },
        ])
        .await;

        let snapshot = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap();

        assert_eq!(snapshot.identity.plan.as_deref(), Some("Pro"));
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("user-agent: antigravity/cli/")
        );
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("user-agent: antigravity/cli/")
        );
        let meta = storage.load_metadata(&account_id).unwrap();
        assert_eq!(
            meta.antigravity_last_tier_id.as_deref(),
            Some("g1-pro-tier")
        );
    }

    #[tokio::test]
    async fn fetch_sends_discovered_project_id_to_quota_endpoint() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body:
                    r#"{"currentTier":{"id":"free-tier"},"cloudaicompanionProject":"daring-goods"}"#
                        .to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: free_quota_body(),
            },
        ])
        .await;

        fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap();

        let requests = handle.await.unwrap();
        assert!(requests[1].contains(r#"{"project":"daring-goods"}"#));
    }

    #[tokio::test]
    async fn fetch_omits_project_when_load_code_assist_returns_none() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"free-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: free_quota_body(),
            },
        ])
        .await;

        fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap();

        let requests = handle.await.unwrap();
        assert!(requests[1].ends_with("{}"));
    }

    #[tokio::test]
    async fn fetch_free_tier_builds_two_weekly_windows_with_weekly_headline() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body:
                    r#"{"currentTier":{"id":"free-tier"},"cloudaicompanionProject":"daring-goods"}"#
                        .to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: free_quota_body(),
            },
        ])
        .await;

        let snapshot = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap();

        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].group.as_deref(), Some("Gemini Models"));
        assert_eq!(
            snapshot.windows[1].group.as_deref(),
            Some("Claude and GPT models")
        );
        assert!(
            snapshot
                .windows
                .iter()
                .all(|window| window.window_seconds == Some(quota::WEEKLY_WINDOW_SECONDS))
        );
        assert_eq!(snapshot.headline, UsageHeadline(0));
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn fetch_preflight_refreshes_and_persists_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() - Duration::minutes(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/token",
                status: 200,
                body: r#"{"access_token":"fresh","expires_in":3600,"scope":"openid"}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"g1-pro-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: quota_body(),
            },
        ])
        .await;

        let snapshot = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap();
        assert_eq!(snapshot.identity.plan.as_deref(), Some("Pro"));
        handle.await.unwrap();
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "fresh");
        assert_eq!(tokens.refresh_token, "old-refresh");
    }

    #[tokio::test]
    async fn fetch_reactively_refreshes_on_401_then_retries() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/token",
                status: 200,
                body: r#"{"access_token":"fresh","expires_in":3600,"scope":"openid"}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"g1-ultra-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: quota_body(),
            },
        ])
        .await;

        let snapshot = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap();
        assert_eq!(snapshot.identity.plan.as_deref(), Some("Ultra"));
        handle.await.unwrap();
        assert_eq!(
            storage.load_tokens(&account_id).unwrap().access_token,
            "fresh"
        );
    }

    #[tokio::test]
    async fn fetch_second_401_surfaces_unauthorized() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 401,
                body: "{}".to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/token",
                status: 200,
                body: r#"{"access_token":"fresh","expires_in":3600,"scope":"openid"}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/load",
                status: 401,
                body: "{}".to_string(),
            },
        ])
        .await;
        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, AntigravityError::Unauthorized));
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn fetch_refresh_invalid_grant_is_permanent() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() - Duration::minutes(1));
        let (base, handle) = server(vec![MockResponse {
            method: "POST",
            path: "/token",
            status: 400,
            body: r#"{"error":"invalid_grant"}"#.to_string(),
        }])
        .await;
        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            AntigravityError::TokenRefreshHttp { status: 400 }
        ));
        assert!(error.requires_user_action());
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn fetch_empty_groups_is_no_usage_data() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"free-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 200,
                body: r#"{"groups":[]}"#.to_string(),
            },
        ])
        .await;
        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, AntigravityError::NoUsageData));
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn fetch_quota_5xx_is_transient() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"free-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "POST",
                path: "/quota",
                status: 503,
                body: "{}".to_string(),
            },
        ])
        .await;
        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            endpoints(&base),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, AntigravityError::QuotaHttp { status: 503 }));
        assert!(error.is_transient());
        handle.await.unwrap();
    }

    #[test]
    fn handle_round_trips_through_provider_account_handle() {
        let now = Utc::now();
        let managed = crate::config::ManagedAntigravityAccountConfig {
            id: "antigravity-test".to_string(),
            label: "user@example.com".to_string(),
            account_root: PathBuf::from("/tmp/yapcap/antigravity-test"),
            email: "user@example.com".to_string(),
            sub: "1".to_string(),
            last_tier_id: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        };
        let handle = ProviderAccountHandle::Antigravity(managed.clone());
        match handle {
            ProviderAccountHandle::Antigravity(round_tripped) => {
                assert_eq!(round_tripped, managed);
            }
            _ => panic!("expected Antigravity variant"),
        }
    }
}
