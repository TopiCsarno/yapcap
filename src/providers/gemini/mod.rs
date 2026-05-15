// SPDX-License-Identifier: MPL-2.0

mod account;
#[allow(dead_code)]
pub mod buckets;
mod code_assist;
mod host_session;
pub mod id_token;
mod login;
pub mod oauth;
pub mod plan_label;

pub(crate) use host_session::system_active_account_id;

use crate::account_storage::{ProviderAccountStorage, ProviderAccountTokens};
use crate::error::{GeminiError, Result};
use crate::model::{ProviderId, ProviderIdentity, UsageHeadline, UsageSnapshot};
use chrono::Utc;
use std::path::PathBuf;
use tracing::warn;

pub use account::{apply_login_account, sync_managed_accounts};
pub use login::{
    GeminiLoginEvent, GeminiLoginState, GeminiLoginStatus, prepare, prepare_for_reauth,
};

struct FetchEndpoints<'a> {
    load_code_assist: &'a str,
    retrieve_user_quota: &'a str,
    token: &'a str,
    cloud_resource_manager: &'a str,
}

pub async fn fetch(
    client: &reqwest::Client,
    account_id: &str,
    account_dir: PathBuf,
    hd: Option<String>,
) -> Result<UsageSnapshot, GeminiError> {
    fetch_at(
        client,
        account_id,
        account_dir,
        hd,
        FetchEndpoints {
            load_code_assist: code_assist::LOAD_CODE_ASSIST_URL,
            retrieve_user_quota: code_assist::RETRIEVE_USER_QUOTA_URL,
            token: oauth::TOKEN_ENDPOINT,
            cloud_resource_manager: code_assist::CLOUD_RESOURCE_MANAGER_PROJECTS_URL,
        },
    )
    .await
}

async fn fetch_at(
    client: &reqwest::Client,
    account_id: &str,
    account_dir: PathBuf,
    hd: Option<String>,
    endpoints: FetchEndpoints<'_>,
) -> Result<UsageSnapshot, GeminiError> {
    let FetchEndpoints {
        load_code_assist: load_code_assist_endpoint,
        retrieve_user_quota: retrieve_user_quota_endpoint,
        token: token_endpoint,
        cloud_resource_manager: cloud_resource_manager_endpoint,
    } = endpoints;
    let root = account_dir
        .parent()
        .ok_or_else(|| GeminiError::AccountStorage("invalid account dir path".to_string()))?;
    let storage = ProviderAccountStorage::new(root);

    let now = Utc::now();
    let mut tokens = storage
        .load_tokens(account_id)
        .map_err(|e| GeminiError::AccountStorage(e.to_string()))?;

    if oauth::needs_refresh(tokens.expires_at, now) {
        let refreshed =
            oauth::refresh_access_token_at(client, token_endpoint, &tokens.refresh_token, now)
                .await?;
        tokens = refreshed_tokens(&refreshed);
        let _ = storage.save_tokens(account_id, &tokens);
    }

    let mut refreshed_once = false;
    let load = match code_assist::load_code_assist_typed(
        client,
        load_code_assist_endpoint,
        &tokens.access_token,
    )
    .await
    {
        Ok(load) => load,
        Err(GeminiError::Unauthorized) => {
            tokens =
                reactive_refresh(client, token_endpoint, &storage, account_id, &tokens).await?;
            refreshed_once = true;
            code_assist::load_code_assist_typed(
                client,
                load_code_assist_endpoint,
                &tokens.access_token,
            )
            .await
            .map_err(|e| match e {
                GeminiError::Unauthorized => GeminiError::Unauthorized,
                other => other,
            })?
        }
        Err(other) => return Err(other),
    };

    let project_id = match load.cloudaicompanion_project.clone() {
        Some(value) => value,
        None => {
            match code_assist::fetch_gen_lang_client_project(
                client,
                cloud_resource_manager_endpoint,
                &tokens.access_token,
            )
            .await?
            {
                Some(found) => found,
                None => return Err(GeminiError::NoCloudaicompanionProject),
            }
        }
    };
    let tier_id = load.tier_id.clone().unwrap_or_default();

    let quota = match code_assist::retrieve_user_quota_typed(
        client,
        retrieve_user_quota_endpoint,
        &tokens.access_token,
        &project_id,
    )
    .await
    {
        Ok(quota) => quota,
        Err(GeminiError::Unauthorized) if !refreshed_once => {
            tokens =
                reactive_refresh(client, token_endpoint, &storage, account_id, &tokens).await?;
            code_assist::retrieve_user_quota_typed(
                client,
                retrieve_user_quota_endpoint,
                &tokens.access_token,
                &project_id,
            )
            .await?
        }
        Err(other) => return Err(other),
    };

    let now = Utc::now();
    let family_windows = buckets::classify_buckets(&quota, &tier_id, now);
    if family_windows.is_empty() {
        return Err(GeminiError::NoUsageData);
    }
    let windows: Vec<_> = family_windows
        .iter()
        .map(buckets::FamilyUsageWindow::to_usage_window)
        .collect();

    let metadata = storage
        .load_metadata(account_id)
        .map_err(|e| GeminiError::AccountStorage(e.to_string()))?;
    let email = Some(metadata.email.clone()).filter(|email| !email.is_empty());
    let plan =
        plan_label::plan_label(&tier_id, hd.as_deref().is_some_and(|s| !s.is_empty())).to_string();

    let mut updated_metadata = metadata.clone();
    updated_metadata.gemini_last_tier_id = load.tier_id.clone();
    updated_metadata.gemini_last_cloudaicompanion_project = Some(project_id.clone());
    updated_metadata.updated_at = now;
    if let Err(error) = storage.save_metadata(account_id, &updated_metadata) {
        warn!(account_id, error = %error, "gemini metadata save failed");
    }

    let snapshot = UsageSnapshot {
        provider: ProviderId::Gemini,
        source: "OAuth".to_string(),
        updated_at: now,
        headline: UsageHeadline::first_available(&windows),
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

async fn reactive_refresh(
    client: &reqwest::Client,
    token_endpoint: &str,
    storage: &ProviderAccountStorage,
    account_id: &str,
    previous: &ProviderAccountTokens,
) -> Result<ProviderAccountTokens, GeminiError> {
    let refreshed =
        oauth::refresh_access_token_at(client, token_endpoint, &previous.refresh_token, Utc::now())
            .await?;
    let new_tokens = refreshed_tokens(&refreshed);
    let _ = storage.save_tokens(account_id, &new_tokens);
    Ok(new_tokens)
}

fn refreshed_tokens(response: &oauth::GeminiRefreshedTokens) -> ProviderAccountTokens {
    ProviderAccountTokens {
        access_token: response.access_token.clone(),
        refresh_token: response.refresh_token.clone(),
        expires_at: response.expires_at,
        scope: response.scope.clone(),
        token_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::ManagedGeminiAccountConfig;
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
                let status_text = if response.status == 200 { "OK" } else { "ERR" };
                let raw = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status,
                    status_text,
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

    fn quota_body() -> String {
        let future = Utc::now() + Duration::hours(6);
        format!(
            r#"{{"buckets":[
                {{"modelId":"gemini-2.5-flash","remainingFraction":0.6,"resetTime":"{}","tokenType":"REQUESTS"}},
                {{"modelId":"gemini-2.5-flash-lite","remainingFraction":0.9,"resetTime":"{}","tokenType":"REQUESTS"}}
            ]}}"#,
            future.to_rfc3339(),
            future.to_rfc3339()
        )
    }

    fn load_body(tier: &str, project: &str) -> String {
        format!(r#"{{"currentTier":{{"id":"{tier}"}},"cloudaicompanionProject":"{project}"}}"#)
    }

    fn create_account(
        storage: &ProviderAccountStorage,
        expires_at: chrono::DateTime<Utc>,
    ) -> (String, PathBuf) {
        let stored = storage
            .create_account(NewProviderAccount {
                provider: ProviderId::Gemini,
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
    async fn fetch_chains_load_and_quota_and_classifies_free_tier() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: load_body("free-tier", "proj-1"),
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
            None,
            FetchEndpoints {
                load_code_assist: &format!("{base_url}/load"),
                retrieve_user_quota: &format!("{base_url}/quota"),
                token: &format!("{base_url}/token"),
                cloud_resource_manager: &format!("{base_url}/projects"),
            },
        )
        .await
        .unwrap();

        assert_eq!(snapshot.provider, ProviderId::Gemini);
        let labels: Vec<_> = snapshot.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, vec!["Flash", "Lite"]);
        assert_eq!(snapshot.identity.email.as_deref(), Some("user@example.com"));
        assert_eq!(snapshot.identity.plan.as_deref(), Some("Free"));
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains("\"project\":\"proj-1\""));
        let meta = storage.load_metadata(&account_id).unwrap();
        assert_eq!(meta.gemini_last_tier_id.as_deref(), Some("free-tier"));
        assert_eq!(
            meta.gemini_last_cloudaicompanion_project.as_deref(),
            Some("proj-1")
        );
    }

    #[tokio::test]
    async fn fetch_reactively_refreshes_on_401_then_retries() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
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
                body: load_body("standard-tier", "proj-paid"),
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
            Some("example.com".to_string()),
            FetchEndpoints {
                load_code_assist: &format!("{base_url}/load"),
                retrieve_user_quota: &format!("{base_url}/quota"),
                token: &format!("{base_url}/token"),
                cloud_resource_manager: &format!("{base_url}/projects"),
            },
        )
        .await
        .unwrap();
        assert_eq!(snapshot.identity.plan.as_deref(), Some("Workspace"));
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 4);
        let tokens = storage.load_tokens(&account_id).unwrap();
        assert_eq!(tokens.access_token, "fresh");
    }

    #[tokio::test]
    async fn fetch_falls_back_to_cloudresourcemanager_when_project_missing() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"free-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "GET",
                path: "/projects",
                status: 200,
                body: r#"{"projects":[
                    {"projectId":"unrelated","lifecycleState":"ACTIVE"},
                    {"projectId":"gen-lang-client-0001234567","lifecycleState":"ACTIVE"}
                ]}"#
                .to_string(),
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
            None,
            FetchEndpoints {
                load_code_assist: &format!("{base_url}/load"),
                retrieve_user_quota: &format!("{base_url}/quota"),
                token: &format!("{base_url}/token"),
                cloud_resource_manager: &format!("{base_url}/projects"),
            },
        )
        .await
        .unwrap();
        assert_eq!(snapshot.provider, ProviderId::Gemini);
        let requests = handle.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[2].contains("\"project\":\"gen-lang-client-0001234567\""));
        let meta = storage.load_metadata(&account_id).unwrap();
        assert_eq!(
            meta.gemini_last_cloudaicompanion_project.as_deref(),
            Some("gen-lang-client-0001234567")
        );
    }

    #[tokio::test]
    async fn fetch_returns_actionable_error_when_neither_path_has_project() {
        let temp = tempfile::tempdir().unwrap();
        let storage = ProviderAccountStorage::new(temp.path());
        let (account_id, account_dir) = create_account(&storage, Utc::now() + Duration::hours(1));
        let (base_url, handle) = server(vec![
            MockResponse {
                method: "POST",
                path: "/load",
                status: 200,
                body: r#"{"currentTier":{"id":"free-tier"}}"#.to_string(),
            },
            MockResponse {
                method: "GET",
                path: "/projects",
                status: 200,
                body: r#"{"projects":[]}"#.to_string(),
            },
        ])
        .await;

        let error = fetch_at(
            &reqwest::Client::new(),
            &account_id,
            account_dir,
            None,
            FetchEndpoints {
                load_code_assist: &format!("{base_url}/load"),
                retrieve_user_quota: &format!("{base_url}/quota"),
                token: &format!("{base_url}/token"),
                cloud_resource_manager: &format!("{base_url}/projects"),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, GeminiError::NoCloudaicompanionProject));
        assert!(error.to_string().contains("gemini"));
        handle.await.unwrap();
    }

    fn sample_managed_config() -> ManagedGeminiAccountConfig {
        let now = Utc::now();
        ManagedGeminiAccountConfig {
            id: "gemini-test".to_string(),
            label: "user@example.com".to_string(),
            account_root: PathBuf::from("/tmp/yapcap/gemini-test"),
            email: "user@example.com".to_string(),
            sub: "1234567890".to_string(),
            hd: None,
            last_tier_id: None,
            last_cloudaicompanion_project: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }
    }

    #[test]
    fn handle_round_trips_through_provider_account_handle() {
        let managed = sample_managed_config();
        let handle = ProviderAccountHandle::Gemini(managed.clone());
        match handle {
            ProviderAccountHandle::Gemini(round_tripped) => {
                assert_eq!(round_tripped, managed);
            }
            _ => panic!("expected Gemini variant"),
        }
    }
}
