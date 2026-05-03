// SPDX-License-Identifier: MPL-2.0

mod discovery;
mod identity;
mod maintenance;
mod refresh;
mod scan;
mod storage;

pub use refresh::fetch;
pub use scan::{CursorScanResult, CursorScanState, confirm_scan, scan};

pub use identity::{find_managed_account, managed_account_id};

pub(crate) use discovery::discover_accounts;
pub(crate) use identity::{managed_config_id, normalized_email};
pub(crate) use maintenance::{sync_managed_accounts, upsert_managed_account};
pub(crate) use scan::default_state_db_path;

use crate::account_storage::ProviderAccountStorage;
use crate::config::ManagedCursorAccountConfig;
use std::path::Path;

pub(crate) fn system_active_account_id(
    managed_accounts: &[ManagedCursorAccountConfig],
    storage: &ProviderAccountStorage,
    db_path: &Path,
) -> Option<String> {
    let (access_token, _) = scan::read_state_vscdb(db_path).ok()?;
    let (user_id, _) = scan::decode_jwt(&access_token).ok()?;
    managed_accounts.iter().find_map(|account| {
        let tokens = storage.load_tokens(&account.id).ok()?;
        if tokens.token_id.as_deref() == Some(user_id.as_str()) {
            Some(managed_account_id(&account.id))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_storage::{NewProviderAccount, ProviderAccountTokens};
    use crate::config::paths;
    use crate::model::ProviderId;
    use crate::test_support;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use chrono::{TimeZone, Utc};
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::NamedTempFile;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("yapcap-cursor-mod-{name}-{nanos}"))
    }

    fn make_jwt(sub: &str, exp: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"HS256\"}");
        let payload =
            URL_SAFE_NO_PAD.encode(format!("{{\"sub\":\"{sub}\",\"exp\":{exp}}}").as_bytes());
        format!("{header}.{payload}.fakesig")
    }

    fn create_db_with_jwt(jwt: &str) -> NamedTempFile {
        let file = NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)")
            .unwrap();
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params!["cursorAuth/accessToken", jwt],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params!["cursorAuth/refreshToken", "refresh-tok"],
        )
        .unwrap();
        file
    }

    #[test]
    fn system_active_matches_by_token_id() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("system-active");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let exp = Utc::now().timestamp() + 3600;
        let jwt = make_jwt("auth0|user_xyz", exp);
        let db = create_db_with_jwt(&jwt);

        let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
        storage
            .replace_account(
                "stor-1".to_string(),
                NewProviderAccount {
                    provider: ProviderId::Cursor,
                    email: "user@example.com".to_string(),
                    provider_account_id: None,
                    organization_id: None,
                    organization_name: None,
                    tokens: ProviderAccountTokens {
                        access_token: jwt.clone(),
                        refresh_token: "refresh-tok".to_string(),
                        expires_at: Utc.timestamp_opt(exp, 0).single().unwrap(),
                        scope: Vec::new(),
                        token_id: Some("user_xyz".to_string()),
                    },
                    snapshot: None,
                },
            )
            .unwrap();

        let managed = vec![ManagedCursorAccountConfig {
            id: "stor-1".to_string(),
            email: "user@example.com".to_string(),
            label: "user@example.com".to_string(),
            account_root: paths().cursor_accounts_dir.join("stor-1"),
            display_name: None,
            plan: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }];

        let result = system_active_account_id(&managed, &storage, db.path());
        assert_eq!(result.as_deref(), Some("cursor-managed:stor-1"));

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }

    #[test]
    fn system_active_returns_none_when_token_id_mismatch() {
        let _guard = test_support::env_lock();
        let state_root = test_dir("system-active-mismatch");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state_root);
        }

        let exp = Utc::now().timestamp() + 3600;
        let jwt = make_jwt("auth0|user_xyz", exp);
        let db = create_db_with_jwt(&jwt);

        let storage = ProviderAccountStorage::new(paths().cursor_accounts_dir);
        storage
            .replace_account(
                "stor-2".to_string(),
                NewProviderAccount {
                    provider: ProviderId::Cursor,
                    email: "other@example.com".to_string(),
                    provider_account_id: None,
                    organization_id: None,
                    organization_name: None,
                    tokens: ProviderAccountTokens {
                        access_token: "some-token".to_string(),
                        refresh_token: "rtoken".to_string(),
                        expires_at: Utc.timestamp_opt(exp, 0).single().unwrap(),
                        scope: Vec::new(),
                        token_id: Some("different_user".to_string()),
                    },
                    snapshot: None,
                },
            )
            .unwrap();

        let managed = vec![ManagedCursorAccountConfig {
            id: "stor-2".to_string(),
            email: "other@example.com".to_string(),
            label: "other@example.com".to_string(),
            account_root: paths().cursor_accounts_dir.join("stor-2"),
            display_name: None,
            plan: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_authenticated_at: None,
        }];

        let result = system_active_account_id(&managed, &storage, db.path());
        assert!(result.is_none());

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }
}
