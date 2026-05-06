// SPDX-License-Identifier: MPL-2.0

use crate::account_storage::{NewProviderAccount, ProviderAccountStorage, ProviderAccountTokens};
use crate::config::{ManagedCursorAccountConfig, host_user_home_dir, paths};
use crate::error::CursorError;
use crate::model::ProviderId;
use crate::providers::cursor::identity::normalized_email;
use crate::providers::cursor::storage::managed_account_dir;
use crate::providers::cursor::storage::new_account_id;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub(crate) const REFRESH_ENDPOINT: &str = "https://api2.cursor.sh/oauth/token";
const CURSOR_CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";
const IDENTITY_ENDPOINT: &str = "https://cursor.com/api/auth/me";

#[derive(Debug, Deserialize)]
struct TokenRefreshResponse {
    access_token: Option<String>,
    #[serde(rename = "shouldLogout", default)]
    should_logout: bool,
}

#[derive(Debug, Deserialize)]
struct ScanIdentityResponse {
    email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CursorScanResult {
    pub email: String,
    pub plan: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub user_id: String,
}

#[derive(Debug, Clone)]
pub enum CursorScanState {
    Idle,
    Scanning,
    Found { email: String, plan: Option<String> },
    AlreadyConnected { email: String },
    Error(String),
}

pub fn default_state_db_path() -> Option<PathBuf> {
    host_user_home_dir().map(|d| d.join(".config/Cursor/User/globalStorage/state.vscdb"))
}

pub(crate) fn read_state_vscdb(path: &Path) -> Result<(String, String), CursorError> {
    if !path.exists() {
        return Err(CursorError::StateDbNotFound {
            path: path.to_owned(),
        });
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(CursorError::StateDbOpen)?;
    let mut stmt = conn
        .prepare(
            "SELECT key, value FROM ItemTable WHERE key IN \
             ('cursorAuth/accessToken', 'cursorAuth/refreshToken')",
        )
        .map_err(CursorError::StateDbQuery)?;
    let rows: Result<Vec<(String, String)>, _> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(CursorError::StateDbQuery)?
        .collect();
    let rows = rows.map_err(CursorError::StateDbQuery)?;
    let mut access_token = None;
    let mut refresh_token = None;
    for (key, value) in rows {
        if value.trim().is_empty() {
            continue;
        }
        match key.as_str() {
            "cursorAuth/accessToken" => access_token = Some(value),
            "cursorAuth/refreshToken" => refresh_token = Some(value),
            _ => {}
        }
    }
    let access_token = access_token
        .ok_or_else(|| CursorError::StateDbMissingKey("cursorAuth/accessToken".to_string()))?;
    let refresh_token = refresh_token
        .ok_or_else(|| CursorError::StateDbMissingKey("cursorAuth/refreshToken".to_string()))?;
    Ok((access_token, refresh_token))
}

pub(crate) fn decode_jwt(token: &str) -> Result<(String, DateTime<Utc>), CursorError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(CursorError::JwtWrongSegments { count: parts.len() });
    }
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(CursorError::JwtBase64)?;
    let payload: serde_json::Value =
        serde_json::from_slice(&payload_bytes).map_err(CursorError::JwtNotJson)?;
    let sub = payload["sub"].as_str().ok_or(CursorError::JwtMissingSub)?;
    let user_id = sub.rsplit_once('|').map_or(sub, |(_, id)| id).to_string();
    let exp = payload["exp"].as_i64().ok_or(CursorError::JwtMissingExp)?;
    let expires_at = Utc
        .timestamp_opt(exp, 0)
        .single()
        .ok_or(CursorError::JwtMissingExp)?;
    Ok((user_id, expires_at))
}

pub(crate) fn build_session_cookie(user_id: &str, access_token: &str) -> String {
    format!("WorkosCursorSessionToken={user_id}%3A%3A{access_token}")
}

pub async fn scan(
    client: &reqwest::Client,
    existing_accounts: &[ManagedCursorAccountConfig],
) -> (CursorScanState, Option<CursorScanResult>) {
    let Some(db_path) = default_state_db_path() else {
        return (
            CursorScanState::Error("Cannot determine Cursor configuration path".to_string()),
            None,
        );
    };
    match scan_inner(client, existing_accounts, &db_path).await {
        Ok(result) => result,
        Err(error) => (CursorScanState::Error(scan_error_message(&error)), None),
    }
}

fn scan_error_message(error: &CursorError) -> String {
    match error {
        CursorError::StateDbNotFound { .. } => {
            "No Cursor account detected. Make sure Cursor IDE is installed and you're logged in."
                .to_string()
        }
        CursorError::StateDbMissingKey(_) => {
            "No Cursor account detected. Make sure you're logged in to Cursor IDE.".to_string()
        }
        CursorError::Unauthorized
        | CursorError::TokenRefreshLogout
        | CursorError::TokenRefreshFailed { status: 400..=499 } => {
            "Cursor session expired. Log in to Cursor IDE and scan again.".to_string()
        }
        _ => error.to_string(),
    }
}

async fn scan_inner(
    client: &reqwest::Client,
    existing_accounts: &[ManagedCursorAccountConfig],
    db_path: &Path,
) -> Result<(CursorScanState, Option<CursorScanResult>), CursorError> {
    let (mut access_token, refresh_token) = read_state_vscdb(db_path)?;
    let (user_id, mut expires_at) = decode_jwt(&access_token)?;

    if expires_at <= Utc::now() {
        let (new_token, new_expiry) = refresh_access_token(client, &refresh_token).await?;
        access_token = new_token;
        expires_at = new_expiry;
    }

    let cookie = build_session_cookie(&user_id, &access_token);
    let email = fetch_scan_email(client, &cookie).await?;

    let scan_result = CursorScanResult {
        email: email.clone(),
        plan: None,
        access_token,
        refresh_token,
        expires_at,
        user_id,
    };

    let state = if existing_accounts.iter().any(|a| a.email == email) {
        CursorScanState::AlreadyConnected { email }
    } else {
        CursorScanState::Found { email, plan: None }
    };

    Ok((state, Some(scan_result)))
}

pub(crate) async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, DateTime<Utc>), CursorError> {
    refresh_access_token_at(client, refresh_token, REFRESH_ENDPOINT).await
}

pub(crate) async fn refresh_access_token_at(
    client: &reqwest::Client,
    refresh_token: &str,
    endpoint: &str,
) -> Result<(String, DateTime<Utc>), CursorError> {
    let response = client
        .post(endpoint)
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CURSOR_CLIENT_ID,
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(CursorError::TokenRefreshRequest)?;

    let status = response.status();
    if !status.is_success() {
        return Err(CursorError::TokenRefreshFailed {
            status: status.as_u16(),
        });
    }

    let refresh_response: TokenRefreshResponse = response
        .json()
        .await
        .map_err(CursorError::TokenRefreshDecode)?;

    if refresh_response.should_logout {
        return Err(CursorError::TokenRefreshLogout);
    }

    let new_access_token = refresh_response
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or(CursorError::TokenRefreshLogout)?;

    let (_, new_expires_at) = decode_jwt(&new_access_token)?;
    Ok((new_access_token, new_expires_at))
}

async fn fetch_scan_email(client: &reqwest::Client, cookie: &str) -> Result<String, CursorError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(cookie).map_err(CursorError::InvalidCookieHeader)?,
    );

    let response = client
        .get(IDENTITY_ENDPOINT)
        .headers(headers)
        .send()
        .await
        .map_err(CursorError::IdentityRequest)?;

    if !response.status().is_success() {
        return Err(CursorError::Unauthorized);
    }

    let identity: ScanIdentityResponse =
        response.json().await.map_err(CursorError::DecodeIdentity)?;

    let email = identity
        .email
        .map(|e| normalized_email(&e))
        .filter(|e| !e.is_empty())
        .ok_or(CursorError::ScanMissingEmail)?;

    Ok(email)
}

pub fn confirm_scan(
    existing_accounts: &[ManagedCursorAccountConfig],
    result: &CursorScanResult,
) -> Result<ManagedCursorAccountConfig, String> {
    let storage_id = existing_accounts
        .iter()
        .find(|a| a.email == result.email)
        .map_or_else(new_account_id, |a| a.id.clone());

    let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
    storage
        .replace_account(
            storage_id.clone(),
            NewProviderAccount {
                provider: ProviderId::Cursor,
                email: result.email.clone(),
                provider_account_id: None,
                organization_id: None,
                organization_name: result.plan.clone(),
                tokens: ProviderAccountTokens {
                    access_token: result.access_token.clone(),
                    refresh_token: result.refresh_token.clone(),
                    expires_at: result.expires_at,
                    scope: Vec::new(),
                    token_id: Some(result.user_id.clone()),
                },
                snapshot: None,
            },
        )
        .map_err(|error| format!("failed to store Cursor account: {error}"))?;

    let now = Utc::now();
    Ok(ManagedCursorAccountConfig {
        id: storage_id.clone(),
        email: result.email.clone(),
        label: result.email.clone(),
        account_root: managed_account_dir(&storage_id),
        display_name: None,
        plan: result.plan.clone(),
        created_at: now,
        updated_at: now,
        last_authenticated_at: Some(now),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::ProviderAccountStorage;
    use crate::test_support;
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::NamedTempFile;

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-{name}-{nanos}"))
    }

    fn create_test_db(entries: &[(&str, &str)]) -> NamedTempFile {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)")
            .unwrap();
        for (k, v) in entries {
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                rusqlite::params![k, v],
            )
            .unwrap();
        }
        file
    }

    fn make_test_jwt(sub: &str, exp: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"HS256\"}");
        let payload =
            URL_SAFE_NO_PAD.encode(format!("{{\"sub\":\"{sub}\",\"exp\":{exp}}}").as_bytes());
        format!("{header}.{payload}.fakesig")
    }

    fn make_scan_result(email: &str) -> CursorScanResult {
        let exp = Utc::now().timestamp() + 3600;
        let jwt = make_test_jwt("auth0|user_abc", exp);
        CursorScanResult {
            email: email.to_string(),
            plan: None,
            access_token: jwt,
            refresh_token: "rtoken".to_string(),
            expires_at: Utc.timestamp_opt(exp, 0).single().unwrap(),
            user_id: "user_abc".to_string(),
        }
    }

    #[test]
    fn reads_both_tokens() {
        let db = create_test_db(&[
            ("cursorAuth/accessToken", "access_tok"),
            ("cursorAuth/refreshToken", "refresh_tok"),
        ]);
        let (access, refresh) = read_state_vscdb(db.path()).unwrap();
        assert_eq!(access, "access_tok");
        assert_eq!(refresh, "refresh_tok");
    }

    #[test]
    fn missing_file_returns_error() {
        let result = read_state_vscdb(Path::new("/nonexistent/state.vscdb"));
        assert!(matches!(result, Err(CursorError::StateDbNotFound { .. })));
    }

    #[test]
    fn missing_refresh_token_returns_error() {
        let db = create_test_db(&[("cursorAuth/accessToken", "access_tok")]);
        let result = read_state_vscdb(db.path());
        assert!(matches!(result, Err(CursorError::StateDbMissingKey(_))));
    }

    #[test]
    fn both_keys_absent_returns_error() {
        let db = create_test_db(&[]);
        let result = read_state_vscdb(db.path());
        assert!(matches!(result, Err(CursorError::StateDbMissingKey(_))));
    }

    #[test]
    fn scan_missing_database_uses_account_detection_message() {
        let error = CursorError::StateDbNotFound {
            path: PathBuf::from("/missing/state.vscdb"),
        };
        let message = scan_error_message(&error);
        assert_eq!(
            message,
            "No Cursor account detected. Make sure Cursor IDE is installed and you're logged in."
        );
    }

    #[test]
    fn scan_missing_access_token_uses_login_message_without_key_name() {
        let message = scan_error_message(&CursorError::StateDbMissingKey(
            "cursorAuth/accessToken".to_string(),
        ));
        assert_eq!(
            message,
            "No Cursor account detected. Make sure you're logged in to Cursor IDE."
        );
        assert!(!message.contains("cursorAuth"));
    }

    #[test]
    fn scan_missing_refresh_token_uses_login_message_without_key_name() {
        let message = scan_error_message(&CursorError::StateDbMissingKey(
            "cursorAuth/refreshToken".to_string(),
        ));
        assert_eq!(
            message,
            "No Cursor account detected. Make sure you're logged in to Cursor IDE."
        );
        assert!(!message.contains("cursorAuth"));
    }

    #[test]
    fn scan_unauthorized_uses_login_and_scan_again_message() {
        let message = scan_error_message(&CursorError::Unauthorized);
        assert_eq!(
            message,
            "Cursor session expired. Log in to Cursor IDE and scan again."
        );
    }

    #[test]
    fn scan_logout_refresh_uses_login_and_scan_again_message() {
        let message = scan_error_message(&CursorError::TokenRefreshLogout);
        assert_eq!(
            message,
            "Cursor session expired. Log in to Cursor IDE and scan again."
        );
    }

    #[test]
    fn decodes_auth0_sub() {
        let token = make_test_jwt("auth0|user_abc", 1_735_689_600);
        let (user_id, expires_at) = decode_jwt(&token).unwrap();
        assert_eq!(user_id, "user_abc");
        assert_eq!(expires_at.timestamp(), 1_735_689_600);
    }

    #[test]
    fn decodes_bare_sub() {
        let token = make_test_jwt("user_xyz", 1_735_689_600);
        let (user_id, _) = decode_jwt(&token).unwrap();
        assert_eq!(user_id, "user_xyz");
    }

    #[test]
    fn rejects_wrong_segment_count() {
        let result = decode_jwt("header.payload");
        assert!(matches!(
            result,
            Err(CursorError::JwtWrongSegments { count: 2 })
        ));
    }

    #[test]
    fn rejects_invalid_base64() {
        let result = decode_jwt("header.!!!.sig");
        assert!(matches!(result, Err(CursorError::JwtBase64(_))));
    }

    #[test]
    fn rejects_missing_sub() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"HS256\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"exp\":1735689600}");
        let result = decode_jwt(&format!("{header}.{payload}.sig"));
        assert!(matches!(result, Err(CursorError::JwtMissingSub)));
    }

    #[test]
    fn rejects_missing_exp() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"HS256\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"sub\":\"user_abc\"}");
        let result = decode_jwt(&format!("{header}.{payload}.sig"));
        assert!(matches!(result, Err(CursorError::JwtMissingExp)));
    }

    #[test]
    fn default_db_path_uses_home_dir() {
        let _guard = test_support::env_lock();
        let fake_home = test_dir("cursor-home");
        unsafe {
            std::env::set_var("HOME", &fake_home);
        }
        let path = default_state_db_path().unwrap();
        assert_eq!(
            path,
            fake_home.join(".config/Cursor/User/globalStorage/state.vscdb")
        );
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn builds_session_cookie() {
        let cookie = build_session_cookie("user_XXX", "token_value");
        assert_eq!(cookie, "WorkosCursorSessionToken=user_XXX%3A%3Atoken_value");
    }

    #[test]
    fn confirm_scan_writes_new_account() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("cursor-confirm-scan");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let result = make_scan_result("user@example.com");
        let account = confirm_scan(&[], &result).unwrap();

        assert_eq!(account.email, "user@example.com");
        assert!(!account.id.is_empty());

        let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
        let tokens = storage.load_tokens(&account.id).unwrap();
        assert_eq!(tokens.access_token, result.access_token);
        assert_eq!(tokens.refresh_token, "rtoken");
        assert_eq!(tokens.token_id.as_deref(), Some("user_abc"));

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }

    #[test]
    fn confirm_scan_updates_existing_account() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("cursor-confirm-scan-update");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let existing_result = make_scan_result("user@example.com");
        let existing_account = confirm_scan(&[], &existing_result).unwrap();

        let updated_result = CursorScanResult {
            access_token: "new_jwt_token".to_string(),
            ..make_scan_result("user@example.com")
        };
        let updated_account = confirm_scan(&[existing_account.clone()], &updated_result).unwrap();

        assert_eq!(updated_account.id, existing_account.id);

        let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
        let tokens = storage.load_tokens(&updated_account.id).unwrap();
        assert_eq!(tokens.access_token, "new_jwt_token");

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }
}
