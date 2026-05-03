// SPDX-License-Identifier: MPL-2.0

use super::*;
use crate::model::{ProviderIdentity, UsageHeadline};
use std::time::{SystemTime, UNIX_EPOCH};

fn test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("yapcap-account-storage-{name}-{nanos}"))
}

fn tokens() -> ProviderAccountTokens {
    ProviderAccountTokens {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        expires_at: Utc::now(),
        scope: vec!["user:profile".to_string()],
        token_id: Some("token-1".to_string()),
    }
}

fn snapshot() -> UsageSnapshot {
    UsageSnapshot {
        provider: ProviderId::Claude,
        source: "claude".to_string(),
        updated_at: Utc::now(),
        headline: UsageHeadline(0),
        windows: Vec::new(),
        provider_cost: None,
        identity: ProviderIdentity {
            email: Some("person@example.com".to_string()),
            account_id: Some("acct-1".to_string()),
            plan: None,
            display_name: None,
        },
    }
}

#[test]
fn creates_account_owned_files_without_using_email_in_directory_name() {
    let storage = ProviderAccountStorage::new(test_dir("create"));
    let token_payload = tokens();
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Claude,
            email: "person@example.com".to_string(),
            provider_account_id: Some("acct-1".to_string()),
            organization_id: Some("org-1".to_string()),
            organization_name: Some("Example Org".to_string()),
            tokens: token_payload.clone(),
            snapshot: Some(snapshot()),
        })
        .unwrap();

    assert!(stored.account_dir.ends_with(&stored.account_ref.account_id));
    assert!(!stored.account_ref.account_id.contains("person"));
    assert!(!stored.account_ref.account_id.contains("example"));
    assert!(stored.account_dir.join(METADATA_FILE).exists());
    assert!(stored.account_dir.join(TOKENS_FILE).exists());
    assert!(stored.account_dir.join(SNAPSHOT_FILE).exists());
    assert_eq!(
        storage
            .load_metadata(&stored.account_ref.account_id)
            .unwrap(),
        stored.metadata
    );
    assert_eq!(
        storage.load_tokens(&stored.account_ref.account_id).unwrap(),
        token_payload
    );
    assert_eq!(
        storage
            .load_snapshot(&stored.account_ref.account_id)
            .unwrap()
            .unwrap()
            .identity
            .email
            .as_deref(),
        Some("person@example.com")
    );
}

#[test]
fn save_snapshot_keeps_snapshot_in_account_directory() {
    let storage = ProviderAccountStorage::new(test_dir("snapshot"));
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Claude,
            email: "person@example.com".to_string(),
            provider_account_id: None,
            organization_id: None,
            organization_name: None,
            tokens: tokens(),
            snapshot: None,
        })
        .unwrap();
    let snapshot = snapshot();

    storage
        .save_snapshot(&stored.account_ref.account_id, &snapshot)
        .unwrap();

    assert_eq!(
        storage
            .load_snapshot(&stored.account_ref.account_id)
            .unwrap(),
        Some(snapshot)
    );
}

#[test]
fn replace_account_updates_tokens_and_metadata_in_existing_directory() {
    let storage = ProviderAccountStorage::new(test_dir("replace"));
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Claude,
            email: "person@example.com".to_string(),
            provider_account_id: Some("acct-1".to_string()),
            organization_id: None,
            organization_name: None,
            tokens: tokens(),
            snapshot: None,
        })
        .unwrap();
    let mut replacement_tokens = tokens();
    replacement_tokens.access_token = "new-access".to_string();

    let replaced = storage
        .replace_account(
            stored.account_ref.account_id.clone(),
            NewProviderAccount {
                provider: ProviderId::Claude,
                email: "person@example.com".to_string(),
                provider_account_id: Some("acct-2".to_string()),
                organization_id: Some("org-2".to_string()),
                organization_name: Some("New Org".to_string()),
                tokens: replacement_tokens.clone(),
                snapshot: None,
            },
        )
        .unwrap();

    assert_eq!(replaced.account_dir, stored.account_dir);
    assert_eq!(replaced.metadata.created_at, stored.metadata.created_at);
    assert_eq!(
        replaced.metadata.provider_account_id.as_deref(),
        Some("acct-2")
    );
    assert_eq!(
        storage
            .load_tokens(&stored.account_ref.account_id)
            .unwrap()
            .access_token,
        "new-access"
    );
}

#[test]
fn account_config_ref_contains_only_provider_and_account_id() {
    let account_ref = ProviderAccountRef {
        provider: ProviderId::Claude,
        account_id: "claude-1".to_string(),
    };

    let serialized = serde_json::to_value(&account_ref).unwrap();

    assert_eq!(
        serialized,
        serde_json::json!({
            "provider": "claude",
            "account_id": "claude-1"
        })
    );
}

#[test]
fn delete_account_removes_account_directory() {
    let storage = ProviderAccountStorage::new(test_dir("delete"));
    let stored = storage
        .create_account(NewProviderAccount {
            provider: ProviderId::Claude,
            email: "person@example.com".to_string(),
            provider_account_id: None,
            organization_id: None,
            organization_name: None,
            tokens: tokens(),
            snapshot: None,
        })
        .unwrap();

    assert!(
        storage
            .delete_account(&stored.account_ref.account_id)
            .unwrap()
    );

    assert!(!stored.account_dir.exists());
    assert!(
        !storage
            .delete_account(&stored.account_ref.account_id)
            .unwrap()
    );
}
