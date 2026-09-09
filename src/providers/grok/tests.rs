// SPDX-License-Identifier: MPL-2.0

use base64::Engine as _;
use sha2::Digest as _;

use super::oauth::{
    PkceCodes, authorization_url, decode_jwt_claims, exchange_code, new_pkce, new_state,
    parse_token_response, refresh_token,
};
use super::usage::parse_billing_snapshot;
use crate::error::GrokError;
use crate::model::{ExtraUsageState, ProviderCost, ProviderId};

#[test]
fn parses_billing_fixture_into_snapshot() {
    let fixture = include_str!("../../../fixtures/grok/billing_response.json");
    let snapshot = parse_billing_snapshot(
        fixture,
        Some("user@x.ai"),
        Some("Grok User"),
        Some("sub-123"),
    )
    .unwrap();
    assert_eq!(snapshot.provider, ProviderId::Grok);
    assert_eq!(snapshot.windows.len(), 1);
    let window = &snapshot.windows[0];
    assert_eq!(window.label, "Weekly");
    assert!((window.used_percent - 46.0).abs() < f32::EPSILON);
    assert_eq!(window.window_seconds, Some(604_800));
    assert!(window.reset_at.is_some());
    assert_eq!(snapshot.identity.plan.as_deref(), Some("SuperGrok"));
    assert_eq!(snapshot.identity.email.as_deref(), Some("user@x.ai"));
    assert_eq!(snapshot.identity.display_name.as_deref(), Some("Grok User"));
    assert_eq!(snapshot.identity.account_id.as_deref(), Some("sub-123"));
    assert_eq!(
        snapshot.provider_cost,
        Some(ProviderCost {
            used: 1500.0,
            limit: None,
            units: "credits".to_string(),
        })
    );
    assert_eq!(snapshot.extra_usage, None);
}

#[test]
fn falls_back_to_grok_build_product_usage() {
    let json = r#"{
        "config": {
            "creditUsagePercent": null,
            "productUsage": [
                { "product": "Other", "usagePercent": 10.0 },
                { "product": "GrokBuild", "usagePercent": 75.5 }
            ]
        },
        "subscriptionTier": "GrokBasic"
    }"#;
    let snapshot = parse_billing_snapshot(json, None, None, None).unwrap();
    assert_eq!(snapshot.windows.len(), 1);
    assert!((snapshot.windows[0].used_percent - 75.5).abs() < f32::EPSILON);
    assert_eq!(snapshot.identity.plan.as_deref(), Some("GrokBasic"));
}

#[test]
fn clamps_used_percentage_to_range() {
    let json_high = r#"{ "config": { "creditUsagePercent": 140.0 } }"#;
    let snapshot_high = parse_billing_snapshot(json_high, None, None, None).unwrap();
    assert!((snapshot_high.windows[0].used_percent - 100.0).abs() < f32::EPSILON);

    let json_low = r#"{ "config": { "creditUsagePercent": -25.0 } }"#;
    let snapshot_low = parse_billing_snapshot(json_low, None, None, None).unwrap();
    assert!((snapshot_low.windows[0].used_percent - 0.0).abs() < f32::EPSILON);
}

#[test]
fn maps_positive_on_demand_cap_to_extra_usage() {
    let json = r#"{
        "config": {
            "creditUsagePercent": 20.0,
            "onDemandCap": { "val": 100.0 },
            "onDemandUsed": { "val": 25.0 }
        }
    }"#;
    let snapshot = parse_billing_snapshot(json, None, None, None).unwrap();
    assert_eq!(
        snapshot.extra_usage,
        Some(ExtraUsageState::Active {
            used_percent: 25.0,
            cost: ProviderCost {
                used: 25.0,
                limit: Some(100.0),
                units: "credits".to_string(),
            },
        })
    );
}

#[test]
fn returns_decode_usage_on_invalid_json() {
    let result = parse_billing_snapshot("not json", None, None, None);
    assert!(matches!(result, Err(GrokError::DecodeUsage(_))));
}

#[test]
fn returns_no_usage_data_when_config_or_percent_missing() {
    let json_no_config = r#"{ "subscriptionTier": "SuperGrok" }"#;
    assert!(matches!(
        parse_billing_snapshot(json_no_config, None, None, None),
        Err(GrokError::NoUsageData)
    ));

    let json_no_percent = r#"{ "config": {} }"#;
    assert!(matches!(
        parse_billing_snapshot(json_no_percent, None, None, None),
        Err(GrokError::NoUsageData)
    ));
}

#[test]
fn returns_invalid_reset_timestamp_on_malformed_end_date() {
    let json = r#"{
        "config": {
            "creditUsagePercent": 10.0,
            "currentPeriod": {
                "end": "invalid-date"
            }
        }
    }"#;
    let result = parse_billing_snapshot(json, None, None, None);
    assert!(matches!(
        result,
        Err(GrokError::InvalidResetTimestamp { .. })
    ));
}

#[test]
fn authorization_url_contains_required_params() {
    let pkce = PkceCodes {
        code_verifier: "verifier123".to_string(),
        code_challenge: "challenge456".to_string(),
    };
    let url = authorization_url("http://127.0.0.1:12345/callback", &pkce, "state789");
    assert!(url.starts_with("https://auth.x.ai/oauth2/authorize"));
    assert!(url.contains("client_id=b1a00492-073a-47ea-816f-4c329264a828"));
    assert!(url.contains("code_challenge=challenge456"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=state789"));
}

#[test]
fn decodes_grok_jwt_claims() {
    let header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    let payload = "eyJzdWIiOiJ1c3ItMSIsImVtYWlsIjoidGVzdGVyQHguYWkiLCJuYW1lIjoiVGVzdGVyIFgiLCJ0ZWFtX2lkIjoidGVhbS0xIn0";
    let token = format!("{header}.{payload}.signature");
    let claims = decode_jwt_claims(&token).unwrap();
    assert_eq!(claims.sub.as_deref(), Some("usr-1"));
    assert_eq!(claims.email.as_deref(), Some("tester@x.ai"));
    assert_eq!(claims.name.as_deref(), Some("Tester X"));
    assert_eq!(claims.team_id.as_deref(), Some("team-1"));
}

#[test]
fn pkce_generates_valid_codes() {
    let pkce = new_pkce();
    assert!(!pkce.code_verifier.is_empty());
    assert!(!pkce.code_challenge.is_empty());
    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(pkce.code_verifier.as_bytes()));
    assert_eq!(pkce.code_challenge, expected);
}

#[test]
fn new_state_generates_32_alphanumeric_chars() {
    let state1 = new_state();
    let state2 = new_state();
    assert_eq!(state1.len(), 32);
    assert!(state1.chars().all(|c| c.is_ascii_alphanumeric()));
    assert_ne!(state1, state2);
}

#[test]
fn decodes_grok_jwt_claims_with_tier() {
    let header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    let payload = "eyJzdWIiOiJ1c3ItMiIsImVtYWlsIjoidGVzdDJAeC5haSIsIm5hbWUiOiJUZXN0ZXIgMiIsInRlYW1faWQiOiJ0ZWFtLTIiLCJ0aWVyIjoiU3VwZXJHcm9rIn0";
    let token = format!("{header}.{payload}.signature");
    let claims = decode_jwt_claims(&token).unwrap();
    assert_eq!(claims.sub.as_deref(), Some("usr-2"));
    assert_eq!(claims.email.as_deref(), Some("test2@x.ai"));
    assert_eq!(claims.name.as_deref(), Some("Tester 2"));
    assert_eq!(claims.team_id.as_deref(), Some("team-2"));
    assert_eq!(claims.tier.as_deref(), Some("SuperGrok"));
}

#[test]
fn decode_jwt_claims_invalid_returns_none() {
    assert_eq!(decode_jwt_claims("not-a-jwt"), None);
    assert_eq!(decode_jwt_claims("a.invalid-base64!@#.c"), None);
    assert_eq!(decode_jwt_claims("a.bm90LWpzb24.c"), None);
}

#[test]
fn parses_valid_token_response() {
    let raw = r#"{
        "access_token": "grok-acc-1",
        "refresh_token": "grok-ref-1",
        "expires_in": 7200,
        "token_type": "Bearer",
        "scope": "openid email"
    }"#;
    let resp = parse_token_response(raw).unwrap();
    assert_eq!(resp.access_token, "grok-acc-1");
    assert_eq!(resp.refresh_token, "grok-ref-1");
    assert_eq!(resp.token_type.as_deref(), Some("Bearer"));
    assert_eq!(resp.scope.as_deref(), Some("openid email"));
    assert!(resp.expires_at > chrono::Utc::now());
}

#[test]
fn parses_token_response_with_timestamp() {
    let raw = r#"{
        "access_token": "grok-acc-2",
        "refresh_token": "grok-ref-2",
        "expires_at": 1800000000
    }"#;
    let resp = parse_token_response(raw).unwrap();
    assert_eq!(resp.access_token, "grok-acc-2");
    assert_eq!(resp.refresh_token, "grok-ref-2");
    assert_eq!(resp.expires_at.timestamp(), 1800000000);
}

#[test]
fn parses_token_response_falls_back_to_jwt_exp() {
    let header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    let payload = "eyJzdWIiOiJ1c3ItMSIsImV4cCI6MTgwMDAwMDAwMH0";
    let access_token = format!("{header}.{payload}.sig");
    let raw = format!(r#"{{"access_token":"{access_token}","refresh_token":"r1"}}"#);
    let resp = parse_token_response(&raw).unwrap();
    assert_eq!(resp.expires_at.timestamp(), 1800000000);
}

#[test]
fn parse_token_response_missing_fields_fail() {
    let missing_access = r#"{"refresh_token":"r1","expires_in":3600}"#;
    assert!(matches!(
        parse_token_response(missing_access),
        Err(GrokError::TokenRefreshParse(_))
    ));

    let missing_refresh = r#"{"access_token":"a1","expires_in":3600}"#;
    assert!(matches!(
        parse_token_response(missing_refresh),
        Err(GrokError::TokenRefreshParse(_))
    ));

    let invalid_expires = r#"{"access_token":"a1","refresh_token":"r1","expires_in":-10}"#;
    assert!(matches!(
        parse_token_response(invalid_expires),
        Err(GrokError::TokenRefreshParse(_))
    ));
}

#[tokio::test]
async fn exchange_code_sends_form_data_and_parses_response() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let token_url = format!("http://{addr}/oauth2/token");

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 4096];
        let bytes = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer)
            .await
            .unwrap();
        let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();

        assert!(request.contains("grant_type=authorization_code"));
        assert!(request.contains("code=auth-code-123"));
        assert!(request.contains("code_verifier=pkce-verifier-456"));
        assert!(request.contains("client_id=b1a00492-073a-47ea-816f-4c329264a828"));

        let body = r#"{"access_token":"acc-new","refresh_token":"ref-new","expires_in":3600}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes())
            .await
            .unwrap();
    });

    let client = reqwest::Client::new();
    let resp = exchange_code(
        &client,
        "auth-code-123",
        "pkce-verifier-456",
        "http://127.0.0.1/cb",
        &token_url,
    )
    .await
    .unwrap();

    assert_eq!(resp.access_token, "acc-new");
    assert_eq!(resp.refresh_token, "ref-new");
    server.await.unwrap();
}

#[tokio::test]
async fn exchange_code_handles_429_retry_after() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let token_url = format!("http://{addr}/oauth2/token");

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let response = "HTTP/1.1 429 Too Many Requests\r\nretry-after: 45\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes())
            .await
            .unwrap();
    });

    let client = reqwest::Client::new();
    let err = exchange_code(&client, "code", "ver", "http://127.0.0.1/cb", &token_url)
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        GrokError::RateLimited {
            retry_after_secs: Some(45)
        }
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn refresh_token_handles_rotation_and_fallback() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let token_url = format!("http://{addr}/oauth2/token");

    let server = tokio::spawn(async move {
        let (mut stream1, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 4096];
        let bytes1 = tokio::io::AsyncReadExt::read(&mut stream1, &mut buffer)
            .await
            .unwrap();
        let request1 = String::from_utf8_lossy(&buffer[..bytes1]).to_string();
        assert!(request1.contains("grant_type=refresh_token"));
        assert!(request1.contains("refresh_token=old-refresh-1"));

        let body1 =
            r#"{"access_token":"rotated-acc","refresh_token":"rotated-ref","expires_in":3600}"#;
        let response1 = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body1}",
            body1.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream1, response1.as_bytes())
            .await
            .unwrap();

        let (mut stream2, _) = listener.accept().await.unwrap();
        let body2 = r#"{"access_token":"reused-acc","expires_in":3600}"#;
        let response2 = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body2}",
            body2.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream2, response2.as_bytes())
            .await
            .unwrap();
    });

    let client = reqwest::Client::new();
    let res1 = refresh_token(&client, "old-refresh-1", &token_url)
        .await
        .unwrap();
    assert_eq!(res1.access_token, "rotated-acc");
    assert_eq!(res1.refresh_token, "rotated-ref");

    let res2 = refresh_token(&client, "fallback-refresh-2", &token_url)
        .await
        .unwrap();
    assert_eq!(res2.access_token, "reused-acc");
    assert_eq!(res2.refresh_token, "fallback-refresh-2");

    server.await.unwrap();
}

#[tokio::test]
async fn refresh_token_handles_http_errors() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let token_url = format!("http://{addr}/oauth2/token");

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let response =
            "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes())
            .await
            .unwrap();
    });

    let client = reqwest::Client::new();
    let err = refresh_token(&client, "ref", &token_url).await.unwrap_err();
    assert!(matches!(err, GrokError::TokenRefreshHttp { status: 500 }));
    server.await.unwrap();
}

#[test]
fn reads_host_credentials_and_matches_active_account() {
    use super::account::{read_host_credentials, system_active_account_id};
    use crate::config::ManagedGrokAccountConfig;
    use chrono::Utc;
    use std::path::PathBuf;

    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
            "key": "test-access-token",
            "refresh_token": "test-refresh-token",
            "expires_at": 1757211101,
            "email": "developer@x.ai",
            "user_id": "usr-999",
            "first_name": "Dev",
            "last_name": "Grok",
            "team_id": "team-888"
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let creds = read_host_credentials(&auth_json).expect("should parse host credentials");
    assert_eq!(creds.access_token, "test-access-token");
    assert_eq!(creds.user_id.as_deref(), Some("usr-999"));
    assert_eq!(creds.email.as_deref(), Some("developer@x.ai"));

    let account = ManagedGrokAccountConfig {
        id: "grok-acc-1".to_string(),
        label: "developer@x.ai".to_string(),
        config_dir: PathBuf::from("/tmp/acc1"),
        email: Some("developer@x.ai".to_string()),
        provider_account_id: Some("usr-999".to_string()),
        team_id: Some("team-888".to_string()),
        plan: Some("SuperGrok".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    };

    let active_id = system_active_account_id(std::slice::from_ref(&account), &auth_json);
    assert_eq!(active_id.as_deref(), Some("grok-acc-1"));
}

#[test]
fn reads_host_credentials_invalid_returns_none() {
    use super::account::read_host_credentials;
    use std::path::Path;

    assert_eq!(
        read_host_credentials(Path::new("/nonexistent/path/auth.json")),
        None
    );

    let temp = tempfile::tempdir().unwrap();
    let not_json = temp.path().join("not_json.json");
    std::fs::write(&not_json, "not-json").unwrap();
    assert_eq!(read_host_credentials(&not_json), None);

    let missing_key = temp.path().join("missing_key.json");
    std::fs::write(&missing_key, r#"{"some_other_key": {}}"#).unwrap();
    assert_eq!(read_host_credentials(&missing_key), None);

    let empty_token = temp.path().join("empty_token.json");
    std::fs::write(
        &empty_token,
        r#"{"https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {"key": "  "}}"#,
    )
    .unwrap();
    assert_eq!(read_host_credentials(&empty_token), None);
}

#[test]
fn reads_host_credentials_with_alternative_key_and_string_expiry() {
    use super::account::read_host_credentials;

    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::custom-client-id": {
            "access_token": "alt-token",
            "expires_at": "1757211101",
            "email": " dev@x.ai ",
            "user_id": " usr-1 "
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let creds = read_host_credentials(&auth_json).expect("should parse alt credentials");
    assert_eq!(creds.access_token, "alt-token");
    assert_eq!(creds.user_id.as_deref(), Some("usr-1"));
    assert_eq!(creds.email.as_deref(), Some("dev@x.ai"));
    assert!(creds.expires_at.is_some());
}

#[test]
fn system_active_account_id_matches_by_email_when_user_id_missing() {
    use super::account::system_active_account_id;
    use crate::config::ManagedGrokAccountConfig;
    use chrono::Utc;
    use std::path::PathBuf;

    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
            "key": "test-key",
            "email": " DEV@x.ai "
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let account = ManagedGrokAccountConfig {
        id: "grok-1".to_string(),
        label: "Grok".to_string(),
        config_dir: PathBuf::from("/tmp/grok-1"),
        email: Some("dev@x.ai".to_string()),
        provider_account_id: None,
        team_id: None,
        plan: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    };

    let active = system_active_account_id(&[account], &auth_json);
    assert_eq!(active.as_deref(), Some("grok-1"));
}

#[test]
fn system_active_account_id_does_not_match_different_user_id() {
    use super::account::system_active_account_id;
    use crate::config::ManagedGrokAccountConfig;
    use chrono::Utc;
    use std::path::PathBuf;

    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
            "key": "test-key",
            "user_id": "usr-1",
            "email": "dev@x.ai"
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let account = ManagedGrokAccountConfig {
        id: "grok-1".to_string(),
        label: "Grok".to_string(),
        config_dir: PathBuf::from("/tmp/grok-1"),
        email: Some("dev@x.ai".to_string()),
        provider_account_id: Some("usr-2".to_string()),
        team_id: None,
        plan: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    };

    let active = system_active_account_id(&[account], &auth_json);
    assert_eq!(active, None);
}

#[test]
fn host_auth_file_path_ends_with_expected_relative_path() {
    use super::account::host_auth_file_path;
    let path = host_auth_file_path();
    if let Some(path) = path {
        assert!(path.ends_with(std::path::Path::new(".grok/auth.json")));
    }
}

#[test]
fn apply_login_account_dedupes_and_selects() {
    use super::account::apply_login_account;
    use crate::config::{Config, ManagedGrokAccountConfig};
    use chrono::Utc;
    use std::path::PathBuf;

    let mut config = Config::default();
    let now = Utc::now();
    let acc1 = ManagedGrokAccountConfig {
        id: "grok-1".to_string(),
        label: "dev@x.ai".to_string(),
        config_dir: PathBuf::from("/tmp/acc1"),
        email: Some("dev@x.ai".to_string()),
        provider_account_id: Some("usr-1".to_string()),
        team_id: None,
        plan: None,
        created_at: now,
        updated_at: now,
        last_authenticated_at: Some(now),
    };

    apply_login_account(&mut config, acc1);
    assert_eq!(config.grok_managed_accounts.len(), 1);
    assert_eq!(config.selected_grok_account_ids, vec!["grok-1".to_string()]);

    let acc2 = ManagedGrokAccountConfig {
        id: "grok-2".to_string(),
        label: "DEV@X.AI".to_string(),
        config_dir: PathBuf::from("/tmp/acc2"),
        email: Some("DEV@X.AI".to_string()),
        provider_account_id: Some("usr-1".to_string()),
        team_id: Some("team-1".to_string()),
        plan: Some("SuperGrok".to_string()),
        created_at: now,
        updated_at: now + chrono::Duration::seconds(10),
        last_authenticated_at: Some(now + chrono::Duration::seconds(10)),
    };

    apply_login_account(&mut config, acc2);
    assert_eq!(config.grok_managed_accounts.len(), 1);
    assert_eq!(config.grok_managed_accounts[0].id, "grok-1");
    assert_eq!(config.selected_grok_account_ids, vec!["grok-1".to_string()]);
    assert_eq!(
        config.grok_managed_accounts[0].id,
        config.selected_grok_account_ids[0]
    );
    assert_eq!(
        config.grok_managed_accounts[0].team_id.as_deref(),
        Some("team-1")
    );
    assert_eq!(
        config.grok_managed_accounts[0].plan.as_deref(),
        Some("SuperGrok")
    );

    let acc_distinct = ManagedGrokAccountConfig {
        id: "grok-distinct".to_string(),
        label: "other@x.ai".to_string(),
        config_dir: PathBuf::from("/tmp/acc-distinct"),
        email: Some("other@x.ai".to_string()),
        provider_account_id: Some("usr-2".to_string()),
        team_id: None,
        plan: None,
        created_at: now,
        updated_at: now,
        last_authenticated_at: Some(now),
    };

    apply_login_account(&mut config, acc_distinct);
    assert_eq!(config.grok_managed_accounts.len(), 2);
    assert_eq!(
        config.selected_grok_account_ids,
        vec!["grok-distinct".to_string()]
    );
    assert_eq!(
        config.grok_managed_accounts[1].id,
        config.selected_grok_account_ids[0]
    );
}

#[test]
fn discover_accounts_reads_storage_and_deduplicates() {
    use super::account::discover_accounts;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use crate::config::{Config, ManagedGrokAccountConfig, managed_grok_account_dir, paths};
    use crate::model::ProviderId;
    use chrono::Utc;
    use std::path::PathBuf;

    let _env = crate::test_support::test_env();
    let storage = ProviderAccountStorage::new(paths().grok_accounts_dir);
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Grok,
            email: "dev@x.ai".to_string(),
            provider_account_id: Some("usr-stored".to_string()),
            organization_id: None,
            organization_name: None,
            tokens: ProviderAccountTokens {
                access_token: "acc".to_string(),
                refresh_token: "ref".to_string(),
                expires_at: Utc::now() + chrono::Duration::hours(1),
                scope: vec![],
                token_id: None,
            },
            snapshot: None,
        })
        .unwrap();

    let account_id = stored.metadata.account_id;
    let now = Utc::now();
    let config = Config {
        grok_managed_accounts: vec![ManagedGrokAccountConfig {
            id: account_id.clone(),
            label: String::new(),
            config_dir: PathBuf::from("/non-canonical/dir"),
            email: None,
            provider_account_id: None,
            team_id: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        selected_grok_account_ids: vec![account_id.clone()],
        ..Config::default()
    };

    let discovered = discover_accounts(&config);
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].id, account_id);
    assert_eq!(discovered[0].label, "dev@x.ai");
    assert_eq!(discovered[0].email.as_deref(), Some("dev@x.ai"));
    assert_eq!(
        discovered[0].provider_account_id.as_deref(),
        Some("usr-stored")
    );
    assert_eq!(
        discovered[0].config_dir,
        managed_grok_account_dir(&account_id)
    );
}

#[test]
fn sync_managed_account_dirs_updates_non_canonical_dirs() {
    use super::account::sync_managed_account_dirs;
    use crate::config::{Config, ManagedGrokAccountConfig, managed_grok_account_dir};
    use chrono::Utc;
    use std::path::PathBuf;

    let _env = crate::test_support::test_env();
    let now = Utc::now();
    let mut config = Config {
        grok_managed_accounts: vec![ManagedGrokAccountConfig {
            id: "grok-sync-1".to_string(),
            label: "dev@x.ai".to_string(),
            config_dir: PathBuf::from("/wrong/path"),
            email: Some("dev@x.ai".to_string()),
            provider_account_id: Some("usr-1".to_string()),
            team_id: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    let changed = sync_managed_account_dirs(&mut config);
    assert!(changed);
    assert_eq!(
        config.grok_managed_accounts[0].config_dir,
        managed_grok_account_dir("grok-sync-1")
    );

    let changed_second = sync_managed_account_dirs(&mut config);
    assert!(!changed_second);
}

#[test]
fn find_matching_account_and_new_account_id() {
    use super::account::{find_matching_account, new_account_id};
    use crate::config::{Config, ManagedGrokAccountConfig};
    use chrono::Utc;
    use std::path::PathBuf;

    let now = Utc::now();
    let config = Config {
        grok_managed_accounts: vec![ManagedGrokAccountConfig {
            id: "grok-find-1".to_string(),
            label: "dev@x.ai".to_string(),
            config_dir: PathBuf::from("/tmp"),
            email: Some("dev@x.ai".to_string()),
            provider_account_id: Some("usr-find-1".to_string()),
            team_id: None,
            plan: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }],
        ..Config::default()
    };

    assert!(find_matching_account(&config, None, Some("usr-find-1")).is_some());
    assert!(find_matching_account(&config, Some("DEV@X.AI"), None).is_some());
    assert!(find_matching_account(&config, Some("other@x.ai"), Some("usr-other")).is_none());
    assert!(find_matching_account(&config, Some("dev@x.ai"), Some("usr-conflict")).is_none());

    let generated_id = new_account_id();
    assert!(generated_id.starts_with("grok-"));
}

struct MockResponse {
    method: &'static str,
    path: &'static str,
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: String,
}

async fn mock_server(
    responses: Vec<MockResponse>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 8192];
            let bytes = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer)
                .await
                .unwrap();
            let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
            assert!(request.starts_with(&format!("{} {}", response.method, response.path)));
            let status_text = match response.status {
                200 => "OK",
                401 => "Unauthorized",
                429 => "Too Many Requests",
                500 => "Internal Server Error",
                _ => "Unknown",
            };
            let mut raw = format!("HTTP/1.1 {} {}\r\n", response.status, status_text);
            for (k, v) in &response.headers {
                raw.push_str(&format!("{k}: {v}\r\n"));
            }
            raw.push_str(&format!(
                "content-length: {}\r\nconnection: close\r\n\r\n{}",
                response.body.len(),
                response.body
            ));
            tokio::io::AsyncWriteExt::write_all(&mut stream, raw.as_bytes())
                .await
                .unwrap();
            requests.push(request);
        }
        requests
    });
    (format!("http://{addr}"), handle)
}

fn setup_test_account(
    storage: &crate::account_storage::ProviderAccountStorage,
    expires_at: chrono::DateTime<chrono::Utc>,
    refresh_token: &str,
) -> (String, std::path::PathBuf) {
    use crate::account_storage::{NewProviderAccount, ProviderAccountTokens};
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Grok,
            email: "dev@x.ai".to_string(),
            provider_account_id: Some("usr-stored".to_string()),
            organization_id: None,
            organization_name: None,
            tokens: ProviderAccountTokens {
                access_token: "old-access".to_string(),
                refresh_token: refresh_token.to_string(),
                expires_at,
                scope: vec!["grok-cli:access".to_string()],
                token_id: None,
            },
            snapshot: None,
        })
        .unwrap();
    (stored.account_ref.account_id, stored.account_dir)
}

#[tokio::test]
async fn fetch_at_refreshes_token_when_expiring_soon() {
    use super::fetch_at;
    use crate::account_storage::ProviderAccountStorage;
    use chrono::{Duration, Utc};

    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) =
        setup_test_account(&storage, Utc::now() + Duration::minutes(2), "old-refresh");

    let billing_body = r#"{
        "config": {
            "creditUsagePercent": 25.0
        },
        "subscriptionTier": "SuperGrok"
    }"#;

    let (base_url, handle) = mock_server(vec![
        MockResponse {
            method: "POST",
            path: "/oauth2/token",
            status: 200,
            headers: vec![("content-type", "application/json")],
            body: r#"{"access_token":"refreshed-acc","refresh_token":"refreshed-ref","expires_in":3600}"#.to_string(),
        },
        MockResponse {
            method: "GET",
            path: "/v1/billing?format=credits",
            status: 200,
            headers: vec![("content-type", "application/json")],
            body: billing_body.to_string(),
        },
    ])
    .await;

    let client = reqwest::Client::new();
    let snapshot = fetch_at(
        &client,
        &account_id,
        account_dir,
        &format!("{base_url}/v1/billing?format=credits"),
        &format!("{base_url}/oauth2/token"),
    )
    .await
    .unwrap();

    assert_eq!(snapshot.provider, ProviderId::Grok);
    assert_eq!(snapshot.windows.len(), 1);
    assert!((snapshot.windows[0].used_percent - 25.0).abs() < f32::EPSILON);
    assert_eq!(snapshot.identity.plan.as_deref(), Some("SuperGrok"));
    assert_eq!(snapshot.identity.email.as_deref(), Some("dev@x.ai"));
    assert_eq!(snapshot.identity.account_id.as_deref(), Some("usr-stored"));

    let requests = handle.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("grant_type=refresh_token"));
    assert!(requests[0].contains("refresh_token=old-refresh"));
    assert!(requests[1].contains("authorization: Bearer refreshed-acc"));

    let updated_tokens = storage.load_tokens(&account_id).unwrap();
    assert_eq!(updated_tokens.access_token, "refreshed-acc");
    assert_eq!(updated_tokens.refresh_token, "refreshed-ref");

    let saved_snapshot = storage.load_snapshot(&account_id).unwrap().unwrap();
    assert!((saved_snapshot.windows[0].used_percent - 25.0).abs() < f32::EPSILON);
}

#[tokio::test]
async fn fetch_at_retries_on_401_with_refresh_and_succeeds() {
    use super::fetch_at;
    use crate::account_storage::ProviderAccountStorage;
    use chrono::{Duration, Utc};

    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) =
        setup_test_account(&storage, Utc::now() + Duration::hours(1), "valid-refresh");

    let billing_body = r#"{
        "config": {
            "creditUsagePercent": 50.0
        },
        "subscriptionTier": "SuperGrok"
    }"#;

    let (base_url, handle) = mock_server(vec![
        MockResponse {
            method: "GET",
            path: "/v1/billing?format=credits",
            status: 401,
            headers: vec![("content-type", "application/json")],
            body: "{}".to_string(),
        },
        MockResponse {
            method: "POST",
            path: "/oauth2/token",
            status: 200,
            headers: vec![("content-type", "application/json")],
            body:
                r#"{"access_token":"retried-acc","refresh_token":"retried-ref","expires_in":3600}"#
                    .to_string(),
        },
        MockResponse {
            method: "GET",
            path: "/v1/billing?format=credits",
            status: 200,
            headers: vec![("content-type", "application/json")],
            body: billing_body.to_string(),
        },
    ])
    .await;

    let client = reqwest::Client::new();
    let snapshot = fetch_at(
        &client,
        &account_id,
        account_dir,
        &format!("{base_url}/v1/billing?format=credits"),
        &format!("{base_url}/oauth2/token"),
    )
    .await
    .unwrap();

    assert!((snapshot.windows[0].used_percent - 50.0).abs() < f32::EPSILON);

    let requests = handle.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("authorization: Bearer old-access"));
    assert!(requests[1].contains("refresh_token=valid-refresh"));
    assert!(requests[2].contains("authorization: Bearer retried-acc"));

    let updated_tokens = storage.load_tokens(&account_id).unwrap();
    assert_eq!(updated_tokens.access_token, "retried-acc");
    assert_eq!(updated_tokens.refresh_token, "retried-ref");

    let saved_snapshot = storage.load_snapshot(&account_id).unwrap().unwrap();
    assert!((saved_snapshot.windows[0].used_percent - 50.0).abs() < f32::EPSILON);
}

#[tokio::test]
async fn fetch_at_returns_unauthorized_if_401_persists_after_refresh() {
    use super::fetch_at;
    use crate::account_storage::ProviderAccountStorage;
    use chrono::{Duration, Utc};

    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) =
        setup_test_account(&storage, Utc::now() + Duration::hours(1), "valid-refresh");

    let (base_url, handle) = mock_server(vec![
        MockResponse {
            method: "GET",
            path: "/v1/billing?format=credits",
            status: 401,
            headers: vec![("content-type", "application/json")],
            body: "{}".to_string(),
        },
        MockResponse {
            method: "POST",
            path: "/oauth2/token",
            status: 200,
            headers: vec![("content-type", "application/json")],
            body:
                r#"{"access_token":"retried-acc","refresh_token":"retried-ref","expires_in":3600}"#
                    .to_string(),
        },
        MockResponse {
            method: "GET",
            path: "/v1/billing?format=credits",
            status: 401,
            headers: vec![("content-type", "application/json")],
            body: "{}".to_string(),
        },
    ])
    .await;

    let client = reqwest::Client::new();
    let err = fetch_at(
        &client,
        &account_id,
        account_dir,
        &format!("{base_url}/v1/billing?format=credits"),
        &format!("{base_url}/oauth2/token"),
    )
    .await
    .unwrap_err();

    assert!(matches!(err, GrokError::Unauthorized));
    assert_eq!(handle.await.unwrap().len(), 3);
}

#[tokio::test]
async fn fetch_at_handles_429_rate_limited() {
    use super::fetch_at;
    use crate::account_storage::ProviderAccountStorage;
    use chrono::{Duration, Utc};

    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) =
        setup_test_account(&storage, Utc::now() + Duration::hours(1), "valid-refresh");

    let (base_url, handle) = mock_server(vec![MockResponse {
        method: "GET",
        path: "/v1/billing?format=credits",
        status: 429,
        headers: vec![("content-type", "application/json"), ("retry-after", "120")],
        body: "{}".to_string(),
    }])
    .await;

    let client = reqwest::Client::new();
    let err = fetch_at(
        &client,
        &account_id,
        account_dir,
        &format!("{base_url}/v1/billing?format=credits"),
        &format!("{base_url}/oauth2/token"),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        GrokError::RateLimited {
            retry_after_secs: Some(120)
        }
    ));
    assert_eq!(handle.await.unwrap().len(), 1);
}

#[tokio::test]
async fn fetch_at_handles_500_endpoint_error() {
    use super::fetch_at;
    use crate::account_storage::ProviderAccountStorage;
    use chrono::{Duration, Utc};

    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) =
        setup_test_account(&storage, Utc::now() + Duration::hours(1), "valid-refresh");

    let (base_url, handle) = mock_server(vec![MockResponse {
        method: "GET",
        path: "/v1/billing?format=credits",
        status: 500,
        headers: vec![("content-type", "application/json")],
        body: "Internal Server Error".to_string(),
    }])
    .await;

    let client = reqwest::Client::new();
    let err = fetch_at(
        &client,
        &account_id,
        account_dir,
        &format!("{base_url}/v1/billing?format=credits"),
        &format!("{base_url}/oauth2/token"),
    )
    .await
    .unwrap_err();

    assert!(matches!(err, GrokError::UsageEndpoint { status: 500, .. }));
    assert_eq!(handle.await.unwrap().len(), 1);
}

#[tokio::test]
async fn fetch_at_fails_if_refresh_token_empty_when_expiring() {
    use super::fetch_at;
    use crate::account_storage::ProviderAccountStorage;
    use chrono::{Duration, Utc};

    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());
    let (account_id, account_dir) =
        setup_test_account(&storage, Utc::now() - Duration::hours(1), "");

    let client = reqwest::Client::new();
    let err = fetch_at(
        &client,
        &account_id,
        account_dir,
        "http://127.0.0.1:9999/billing",
        "http://127.0.0.1:9999/token",
    )
    .await
    .unwrap_err();

    assert!(matches!(err, GrokError::RefreshUnavailable));
}

#[tokio::test]
async fn fetch_at_uses_jwt_claims_when_metadata_missing() {
    use super::fetch_at;
    use crate::account_storage::{
        NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens,
    };
    use chrono::{Duration, Utc};

    let temp = tempfile::tempdir().unwrap();
    let storage = ProviderAccountStorage::new(temp.path());

    let header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    let payload = "eyJzdWIiOiJ1c3Itand0IiwiZW1haWwiOiJqd3RAeC5haSIsIm5hbWUiOiJKV1QgVXNlciJ9";
    let jwt_token = format!("{header}.{payload}.sig");

    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Grok,
            email: String::new(),
            provider_account_id: None,
            organization_id: None,
            organization_name: None,
            tokens: ProviderAccountTokens {
                access_token: jwt_token,
                refresh_token: "ref".to_string(),
                expires_at: Utc::now() + Duration::hours(1),
                scope: vec![],
                token_id: None,
            },
            snapshot: None,
        })
        .unwrap();

    let billing_body = r#"{
        "config": {
            "creditUsagePercent": 10.0
        },
        "subscriptionTier": "SuperGrok"
    }"#;

    let (base_url, handle) = mock_server(vec![MockResponse {
        method: "GET",
        path: "/v1/billing?format=credits",
        status: 200,
        headers: vec![("content-type", "application/json")],
        body: billing_body.to_string(),
    }])
    .await;

    let client = reqwest::Client::new();
    let snapshot = fetch_at(
        &client,
        &stored.account_ref.account_id,
        stored.account_dir,
        &format!("{base_url}/v1/billing?format=credits"),
        &format!("{base_url}/oauth2/token"),
    )
    .await
    .unwrap();

    assert_eq!(snapshot.identity.email.as_deref(), Some("jwt@x.ai"));
    assert_eq!(snapshot.identity.account_id.as_deref(), Some("usr-jwt"));
    assert_eq!(snapshot.identity.display_name.as_deref(), Some("JWT User"));
    assert_eq!(handle.await.unwrap().len(), 1);
}

#[tokio::test]
async fn fetch_forwards_to_default_billing_url() {
    use super::{DEFAULT_BILLING_URL, fetch};
    use std::path::PathBuf;

    assert_eq!(
        DEFAULT_BILLING_URL,
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits"
    );

    let client = reqwest::Client::new();
    let err = fetch(
        &client,
        "non-existent-account",
        PathBuf::from("/nonexistent/grok/non-existent-account"),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        GrokError::CredentialsMissing | GrokError::AccountStorage(_)
    ));
}

#[test]
fn prepare_host_import_creates_running_state() {
    let config = crate::config::Config::default();
    let (state, _task) = super::login::prepare_host_import(config, None).unwrap();
    assert_eq!(state.status, super::login::GrokLoginStatus::Running);
    assert!(state.importing_from_host_cli);
}

#[test]
fn prepare_creates_running_state() {
    let config = crate::config::Config::default();
    let (state, _task) = super::login::prepare(config).unwrap();
    assert_eq!(state.status, super::login::GrokLoginStatus::Running);
    assert!(!state.importing_from_host_cli);
    assert!(state.login_url.is_none());
    assert!(state.error.is_none());
}

#[test]
fn prepare_targeted_with_nonexistent_account_fails() {
    let config = crate::config::Config::default();
    let err = super::login::prepare_targeted("missing-account".to_string(), config).unwrap_err();
    assert!(err.contains("missing-account no longer exists"));
}

#[test]
fn prepare_targeted_with_existing_account_creates_running_state() {
    use crate::config::ManagedGrokAccountConfig;
    use chrono::Utc;
    use std::path::PathBuf;

    let account = ManagedGrokAccountConfig {
        id: "grok-target-1".to_string(),
        label: "Target Account".to_string(),
        config_dir: PathBuf::from("/tmp/target-1"),
        email: Some("target@x.ai".to_string()),
        provider_account_id: Some("usr-target-1".to_string()),
        team_id: None,
        plan: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    };
    let config = crate::config::Config {
        grok_managed_accounts: vec![account],
        ..Default::default()
    };
    let (state, _task) =
        super::login::prepare_targeted("grok-target-1".to_string(), config).unwrap();
    assert_eq!(state.status, super::login::GrokLoginStatus::Running);
    assert!(!state.importing_from_host_cli);
}

#[test]
fn prepare_host_import_with_nonexistent_target_fails() {
    let config = crate::config::Config::default();
    let err =
        super::login::prepare_host_import(config, Some("missing-target".to_string())).unwrap_err();
    assert!(err.contains("missing-target no longer exists"));
}

#[tokio::test]
async fn host_import_reads_credentials_commits_and_fetches_snapshot() {
    use crate::account_storage::ProviderAccountStorage;
    use crate::config::paths;
    use crate::test_support;

    let _env = test_support::test_env();

    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
            "key": "import-access-token",
            "refresh_token": "import-refresh-token",
            "expires_at": 1893456000,
            "email": "imported@x.ai",
            "user_id": "usr-import-123",
            "first_name": "Imported",
            "last_name": "User",
            "team_id": "team-import-456"
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let billing_body = r#"{
        "config": {
            "creditUsagePercent": 50.0
        },
        "subscriptionTier": "SuperGrok"
    }"#;
    let (base_url, _server) = mock_server(vec![MockResponse {
        method: "GET",
        path: "/v1/billing",
        status: 200,
        headers: vec![("content-type", "application/json")],
        body: billing_body.to_string(),
    }])
    .await;

    let client = reqwest::Client::new();
    let billing_url = format!("{base_url}/v1/billing");
    let token_url = format!("{base_url}/oauth2/token");

    let success = super::login::run_host_import_with(
        "grok-import-test".to_string(),
        crate::config::Config::default(),
        None,
        Some(&auth_json),
        &client,
        &billing_url,
        &token_url,
    )
    .await
    .unwrap();

    assert_eq!(success.account.id, "grok-import-test");
    assert_eq!(success.account.email.as_deref(), Some("imported@x.ai"));
    assert_eq!(
        success.account.provider_account_id.as_deref(),
        Some("usr-import-123")
    );
    assert_eq!(success.account.team_id.as_deref(), Some("team-import-456"));
    assert_eq!(success.account.plan.as_deref(), Some("SuperGrok"));

    let storage = ProviderAccountStorage::new(paths().grok_accounts_dir);
    let tokens = storage.load_tokens("grok-import-test").unwrap();
    assert_eq!(tokens.access_token, "import-access-token");
    assert_eq!(tokens.refresh_token, "import-refresh-token");

    let snapshot = storage
        .load_snapshot("grok-import-test")
        .unwrap()
        .expect("snapshot should exist");
    assert_eq!(snapshot.windows.len(), 1);
    assert_eq!(snapshot.windows[0].used_percent, 50.0);
}

#[tokio::test]
async fn host_import_reauth_rejects_mismatched_identity() {
    use crate::config::ManagedGrokAccountConfig;
    use crate::test_support;
    use chrono::Utc;
    use std::path::PathBuf;

    let _env = test_support::test_env();

    let existing = ManagedGrokAccountConfig {
        id: "grok-original".to_string(),
        label: "original@x.ai".to_string(),
        config_dir: PathBuf::from("/tmp/original"),
        email: Some("original@x.ai".to_string()),
        provider_account_id: Some("usr-original".to_string()),
        team_id: None,
        plan: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    };
    let config = crate::config::Config {
        grok_managed_accounts: vec![existing],
        ..Default::default()
    };

    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
            "key": "diff-access-token",
            "email": "different@x.ai",
            "user_id": "usr-different"
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let client = reqwest::Client::new();
    let err = super::login::run_host_import_with(
        "flow-id".to_string(),
        config,
        Some("grok-original".to_string()),
        Some(&auth_json),
        &client,
        "http://127.0.0.1:9/billing",
        "http://127.0.0.1:9/token",
    )
    .await
    .unwrap_err();

    assert!(err.contains("different Grok account"));
}

#[tokio::test]
async fn host_import_reauth_updates_same_account() {
    use crate::account_storage::ProviderAccountStorage;
    use crate::config::{ManagedGrokAccountConfig, paths};
    use crate::test_support;
    use chrono::{Duration, Utc};
    use std::path::PathBuf;

    let _env = test_support::test_env();

    let created_at = Utc::now() - Duration::hours(5);
    let existing = ManagedGrokAccountConfig {
        id: "grok-reauth-target".to_string(),
        label: "Target Account".to_string(),
        config_dir: PathBuf::from("/tmp/target"),
        email: Some("dev@x.ai".to_string()),
        provider_account_id: Some("usr-999".to_string()),
        team_id: None,
        plan: None,
        created_at,
        updated_at: created_at,
        last_authenticated_at: None,
    };
    let config = crate::config::Config {
        grok_managed_accounts: vec![existing],
        ..Default::default()
    };

    let temp = tempfile::tempdir().unwrap();
    let auth_json = temp.path().join("auth.json");
    let json_data = r#"{
        "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
            "key": "new-access-token",
            "refresh_token": "new-refresh-token",
            "email": "dev@x.ai",
            "user_id": "usr-999"
        }
    }"#;
    std::fs::write(&auth_json, json_data).unwrap();

    let client = reqwest::Client::new();
    let success = super::login::run_host_import_with(
        "new-flow-id".to_string(),
        config,
        Some("grok-reauth-target".to_string()),
        Some(&auth_json),
        &client,
        "http://127.0.0.1:9/billing",
        "http://127.0.0.1:9/token",
    )
    .await
    .unwrap();

    assert_eq!(success.account.id, "grok-reauth-target");
    assert_eq!(success.account.created_at, created_at);

    let storage = ProviderAccountStorage::new(paths().grok_accounts_dir);
    let tokens = storage.load_tokens("grok-reauth-target").unwrap();
    assert_eq!(tokens.access_token, "new-access-token");
    assert_eq!(tokens.refresh_token, "new-refresh-token");
}

#[tokio::test]
async fn oauth_callback_exchanges_code_and_sends_success_page() {
    let (base_url, handle) = mock_server(vec![MockResponse {
        method: "POST",
        path: "/oauth2/token",
        status: 200,
        headers: vec![("content-type", "application/json")],
        body: r#"{"access_token":"token-abc","refresh_token":"ref-xyz","expires_in":3600}"#
            .to_string(),
    }])
    .await;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req =
            "GET /callback?code=test-code&state=good-state HTTP/1.1\r\nHost: localhost\r\n\r\n";
        tokio::io::AsyncWriteExt::write_all(&mut stream, req.as_bytes())
            .await
            .unwrap();
        let mut resp = vec![0u8; 4096];
        let bytes = tokio::io::AsyncReadExt::read(&mut stream, &mut resp)
            .await
            .unwrap();
        String::from_utf8_lossy(&resp[..bytes]).to_string()
    });

    let (mut server_stream, _) = listener.accept().await.unwrap();
    let client = reqwest::Client::new();
    let token_url = format!("{base_url}/oauth2/token");
    let tokens = super::login::handle_callback(
        &mut server_stream,
        &client,
        &token_url,
        "http://127.0.0.1/callback",
        "my-verifier",
        "good-state",
    )
    .await
    .unwrap()
    .expect("callback should return tokens");

    let response_text = client_task.await.unwrap();
    assert!(response_text.contains("HTTP/1.1 200 OK"));
    assert!(response_text.contains("YapCap: Grok sign-in complete"));
    assert_eq!(tokens.access_token, "token-abc");
    assert_eq!(tokens.refresh_token, "ref-xyz");

    let server_reqs = handle.await.unwrap();
    assert_eq!(server_reqs.len(), 1);
    assert!(server_reqs[0].contains("code=test-code"));
    assert!(server_reqs[0].contains("code_verifier=my-verifier"));
}

#[tokio::test]
async fn oauth_callback_rejects_state_mismatch() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = "GET /callback?code=test-code&state=wrong-state HTTP/1.1\r\n\r\n";
        tokio::io::AsyncWriteExt::write_all(&mut stream, req.as_bytes())
            .await
            .unwrap();
        let mut resp = vec![0u8; 4096];
        let bytes = tokio::io::AsyncReadExt::read(&mut stream, &mut resp)
            .await
            .unwrap();
        String::from_utf8_lossy(&resp[..bytes]).to_string()
    });

    let (mut server_stream, _) = listener.accept().await.unwrap();
    let client = reqwest::Client::new();
    let err = super::login::handle_callback(
        &mut server_stream,
        &client,
        "http://127.0.0.1:9/token",
        "http://127.0.0.1/callback",
        "my-verifier",
        "expected-state",
    )
    .await
    .unwrap_err();

    assert!(err.contains("state nonce did not match"));
    let response_text = client_task.await.unwrap();
    assert!(response_text.contains("HTTP/1.1 400 Bad Request"));
}

#[tokio::test]
async fn oauth_callback_rejects_oauth_error() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = "GET /callback?error=access_denied&state=expected-state HTTP/1.1\r\n\r\n";
        tokio::io::AsyncWriteExt::write_all(&mut stream, req.as_bytes())
            .await
            .unwrap();
        let mut resp = vec![0u8; 4096];
        let bytes = tokio::io::AsyncReadExt::read(&mut stream, &mut resp)
            .await
            .unwrap();
        String::from_utf8_lossy(&resp[..bytes]).to_string()
    });

    let (mut server_stream, _) = listener.accept().await.unwrap();
    let client = reqwest::Client::new();
    let err = super::login::handle_callback(
        &mut server_stream,
        &client,
        "http://127.0.0.1:9/token",
        "http://127.0.0.1/callback",
        "my-verifier",
        "expected-state",
    )
    .await
    .unwrap_err();

    assert!(err.contains("Grok OAuth returned access_denied"));
    let response_text = client_task.await.unwrap();
    assert!(response_text.contains("HTTP/1.1 400 Bad Request"));
}

#[tokio::test]
async fn oauth_callback_returns_none_for_wrong_path() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = "GET /favicon.ico?foo=bar HTTP/1.1\r\n\r\n";
        tokio::io::AsyncWriteExt::write_all(&mut stream, req.as_bytes())
            .await
            .unwrap();
        let mut resp = vec![0u8; 4096];
        let bytes = tokio::io::AsyncReadExt::read(&mut stream, &mut resp)
            .await
            .unwrap();
        String::from_utf8_lossy(&resp[..bytes]).to_string()
    });

    let (mut server_stream, _) = listener.accept().await.unwrap();
    let client = reqwest::Client::new();
    let result = super::login::handle_callback(
        &mut server_stream,
        &client,
        "http://127.0.0.1:9/token",
        "http://127.0.0.1/callback",
        "my-verifier",
        "expected-state",
    )
    .await
    .unwrap();

    assert!(result.is_none());
    let response_text = client_task.await.unwrap();
    assert!(response_text.contains("HTTP/1.1 404 Not Found"));
}

#[tokio::test]
async fn run_login_inner_executes_oauth_loopback_flow() {
    use crate::account_storage::ProviderAccountStorage;
    use crate::config::paths;
    use crate::test_support;
    use cosmic::iced::futures::StreamExt;

    let _env = test_support::test_env();

    let billing_body = r#"{
        "config": {
            "creditUsagePercent": 33.0
        },
        "subscriptionTier": "SuperGrok"
    }"#;
    let (base_url, _server) = mock_server(vec![
        MockResponse {
            method: "POST",
            path: "/oauth2/token",
            status: 200,
            headers: vec![("content-type", "application/json")],
            body: r#"{"access_token":"login-access-tok","refresh_token":"login-refresh-tok","expires_in":3600}"#.to_string(),
        },
        MockResponse {
            method: "GET",
            path: "/v1/billing",
            status: 200,
            headers: vec![("content-type", "application/json")],
            body: billing_body.to_string(),
        },
    ])
    .await;

    let token_url = format!("{base_url}/oauth2/token");
    let billing_url = format!("{base_url}/v1/billing");

    let (mut sender, mut receiver) = cosmic::iced::futures::channel::mpsc::channel(10);
    let login_task = tokio::spawn(async move {
        super::login::run_login_inner(
            "flow-oauth-test",
            &crate::config::Config::default(),
            None,
            &mut sender,
            &token_url,
            &billing_url,
        )
        .await
    });

    let event = receiver
        .next()
        .await
        .expect("should receive LoginUrl event");
    let auth_url = match event {
        super::login::GrokLoginEvent::LoginUrl { url, .. } => url,
        _ => panic!("expected LoginUrl event"),
    };

    let url_parsed = reqwest::Url::parse(&auth_url).unwrap();
    let redirect_param = url_parsed
        .query_pairs()
        .find(|(k, _)| k == "redirect_uri")
        .unwrap()
        .1
        .to_string();
    let state_param = url_parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();

    let redirect_url = reqwest::Url::parse(&redirect_param).unwrap();
    let port = redirect_url.port().unwrap();

    let callback_resp = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{port}/callback?code=mock-code&state={state_param}"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(callback_resp.status(), reqwest::StatusCode::OK);
    let body = callback_resp.text().await.unwrap();
    assert!(body.contains("YapCap: Grok sign-in complete"));

    let success = login_task
        .await
        .unwrap()
        .expect("login flow should succeed");
    assert_eq!(success.account.id, "flow-oauth-test");
    assert_eq!(success.account.plan.as_deref(), Some("SuperGrok"));

    let storage = ProviderAccountStorage::new(paths().grok_accounts_dir);
    let tokens = storage.load_tokens("flow-oauth-test").unwrap();
    assert_eq!(tokens.access_token, "login-access-tok");
    assert_eq!(tokens.refresh_token, "login-refresh-tok");
}
