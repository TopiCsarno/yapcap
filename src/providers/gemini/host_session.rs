// SPDX-License-Identifier: MPL-2.0

use crate::config::ManagedGeminiAccountConfig;
use crate::providers::gemini::account::normalized_email;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct GoogleAccountsFile {
    #[serde(default)]
    active: Option<String>,
}

pub(crate) fn parse_active_email(contents: &str) -> Option<String> {
    if contents.trim().is_empty() {
        return None;
    }
    let parsed: GoogleAccountsFile = serde_json::from_str(contents).ok()?;
    let active = parsed.active?;
    let normalized = normalized_email(&active);
    (!normalized.is_empty()).then_some(normalized)
}

pub(crate) fn match_account_id(
    managed_accounts: &[ManagedGeminiAccountConfig],
    active_email: &str,
) -> Option<String> {
    managed_accounts
        .iter()
        .find(|account| normalized_email(&account.email) == active_email)
        .map(|account| account.id.clone())
}

pub(crate) fn system_active_account_id(
    managed_accounts: &[ManagedGeminiAccountConfig],
    google_accounts_path: &Path,
) -> Option<String> {
    let contents = std::fs::read_to_string(google_accounts_path).ok()?;
    let active = parse_active_email(&contents)?;
    match_account_id(managed_accounts, &active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    fn managed(id: &str, email: &str) -> ManagedGeminiAccountConfig {
        let now = Utc::now();
        ManagedGeminiAccountConfig {
            id: id.to_string(),
            label: email.to_string(),
            account_root: PathBuf::from(format!("/tmp/{id}")),
            email: email.to_string(),
            sub: "sub".to_string(),
            hd: None,
            last_tier_id: None,
            last_cloudaicompanion_project: None,
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }
    }

    #[test]
    fn parses_well_formed_active() {
        let active =
            parse_active_email(r#"{"active":"User@Example.com","old":["x@y.com"]}"#).unwrap();
        assert_eq!(active, "user@example.com");
    }

    #[test]
    fn returns_none_when_active_missing() {
        assert!(parse_active_email(r#"{"old":["x@y.com"]}"#).is_none());
    }

    #[test]
    fn returns_none_on_malformed_json() {
        assert!(parse_active_email("not json at all").is_none());
    }

    #[test]
    fn returns_none_on_empty_file() {
        assert!(parse_active_email("").is_none());
        assert!(parse_active_email("   \n  ").is_none());
    }

    #[test]
    fn returns_none_when_active_is_empty_string() {
        assert!(parse_active_email(r#"{"active":"   "}"#).is_none());
    }

    #[test]
    fn matches_account_id_by_normalized_email() {
        let accounts = vec![
            managed("gemini-a", "alice@example.com"),
            managed("gemini-b", "BOB@example.com"),
        ];
        assert_eq!(
            match_account_id(&accounts, "bob@example.com").as_deref(),
            Some("gemini-b")
        );
    }

    #[test]
    fn returns_none_when_active_email_untracked() {
        let accounts = vec![managed("gemini-a", "alice@example.com")];
        assert_eq!(match_account_id(&accounts, "stranger@example.com"), None);
    }

    #[test]
    fn returns_none_when_file_absent() {
        let path = PathBuf::from("/nonexistent/yapcap-test/google_accounts.json");
        let accounts = vec![managed("gemini-a", "alice@example.com")];
        assert_eq!(system_active_account_id(&accounts, &path), None);
    }
}
