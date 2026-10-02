use crate::app::AppModel;
use crate::app::login::LoginFlow;
use crate::config::{
    Config, ManagedKimiAccountConfig, ManagedMinimaxAccountConfig, ManagedOpenCodeGoAccountConfig,
    ManagedOpenRouterAccountConfig, ManagedZaiAccountConfig,
};
use crate::key_authentication::KeyAuthenticationState;
use crate::model::ProviderId;
use crate::providers::kimi::storage as kimi_storage;
use crate::providers::minimax::storage as minimax_storage;
use crate::providers::opencode_go::storage as opencode_go_storage;
use crate::providers::openrouter::storage as openrouter_storage;
use crate::providers::zai::storage as zai_storage;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

pub(super) trait KeyAuthenticationCase {
    type Flow: LoginFlow<
            State = KeyAuthenticationState,
            Event = crate::key_authentication::KeyAuthenticationEvent,
        >;

    const PROVIDER: ProviderId;

    fn state(app: &AppModel) -> Option<&KeyAuthenticationState>;
    fn opencode_auth(key: &str) -> String;
    fn storage_root(root: &Path) -> PathBuf;
    fn account_facts(config: &Config, account_id: &str) -> Option<AccountFacts>;
    fn set_account(config: &mut Config, account_id: &str, label: &str, created_at: DateTime<Utc>);
    fn load_api_key(account_id: &str) -> Result<String, String>;
    fn write_api_key(account_id: &str, api_key: &str) -> Result<(), String>;
    fn save_error_prefix() -> &'static str;
}

pub(super) struct KimiCase;

impl KeyAuthenticationCase for KimiCase {
    type Flow = super::super::KimiLoginFlow;

    const PROVIDER: ProviderId = ProviderId::Kimi;

    fn state(app: &AppModel) -> Option<&KeyAuthenticationState> {
        app.kimi_login.as_ref()
    }

    fn opencode_auth(key: &str) -> String {
        format!(r#"{{"kimi-for-coding":{{"type":"api","key":"{key}"}}}}"#)
    }

    fn storage_root(root: &Path) -> PathBuf {
        root.join("yapcap/kimi-accounts")
    }

    fn account_facts(config: &Config, account_id: &str) -> Option<AccountFacts> {
        config
            .kimi_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(AccountFacts::from_kimi)
    }

    fn set_account(config: &mut Config, account_id: &str, label: &str, created_at: DateTime<Utc>) {
        config.kimi_managed_accounts.push(ManagedKimiAccountConfig {
            id: account_id.to_string(),
            label: label.to_string(),
            api_key_source: "stored".to_string(),
            created_at,
            updated_at: created_at,
            last_authenticated_at: Some(created_at),
        });
    }

    fn load_api_key(account_id: &str) -> Result<String, String> {
        kimi_storage::load_api_key(account_id)
    }

    fn write_api_key(account_id: &str, api_key: &str) -> Result<(), String> {
        kimi_storage::write_api_key(account_id, api_key)
    }

    fn save_error_prefix() -> &'static str {
        "Failed to save Kimi API key:"
    }
}

pub(super) struct OpenRouterCase;

impl KeyAuthenticationCase for OpenRouterCase {
    type Flow = super::super::OpenRouterLoginFlow;

    const PROVIDER: ProviderId = ProviderId::OpenRouter;

    fn state(app: &AppModel) -> Option<&KeyAuthenticationState> {
        app.openrouter_login.as_ref()
    }

    fn opencode_auth(key: &str) -> String {
        format!(r#"{{"openrouter":{{"type":"api","key":"{key}"}}}}"#)
    }

    fn storage_root(root: &Path) -> PathBuf {
        root.join("yapcap/openrouter-accounts")
    }

    fn account_facts(config: &Config, account_id: &str) -> Option<AccountFacts> {
        config
            .openrouter_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(AccountFacts::from_openrouter)
    }

    fn set_account(config: &mut Config, account_id: &str, label: &str, created_at: DateTime<Utc>) {
        config
            .openrouter_managed_accounts
            .push(ManagedOpenRouterAccountConfig {
                id: account_id.to_string(),
                label: label.to_string(),
                api_key_source: "stored".to_string(),
                created_at,
                updated_at: created_at,
                last_authenticated_at: Some(created_at),
            });
    }

    fn load_api_key(account_id: &str) -> Result<String, String> {
        openrouter_storage::load_api_key(account_id)
    }

    fn write_api_key(account_id: &str, api_key: &str) -> Result<(), String> {
        openrouter_storage::write_api_key(account_id, api_key)
    }

    fn save_error_prefix() -> &'static str {
        "Failed to save OpenRouter API key:"
    }
}

pub(super) struct MinimaxCase;
impl KeyAuthenticationCase for MinimaxCase {
    type Flow = super::super::MinimaxLoginFlow;

    const PROVIDER: ProviderId = ProviderId::Minimax;

    fn state(app: &AppModel) -> Option<&KeyAuthenticationState> {
        app.minimax_login.as_ref()
    }

    fn opencode_auth(key: &str) -> String {
        format!(r#"{{"minimax":{{"type":"api","key":"{key}"}}}}"#)
    }

    fn storage_root(root: &Path) -> PathBuf {
        root.join("yapcap/minimax-accounts")
    }

    fn account_facts(config: &Config, account_id: &str) -> Option<AccountFacts> {
        config
            .minimax_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(AccountFacts::from_minimax)
    }

    fn set_account(config: &mut Config, account_id: &str, label: &str, created_at: DateTime<Utc>) {
        config
            .minimax_managed_accounts
            .push(ManagedMinimaxAccountConfig {
                id: account_id.to_string(),
                label: label.to_string(),
                api_key_source: "stored".to_string(),
                created_at,
                updated_at: created_at,
                last_authenticated_at: Some(created_at),
            });
    }

    fn load_api_key(account_id: &str) -> Result<String, String> {
        minimax_storage::load_api_key(account_id)
    }

    fn write_api_key(account_id: &str, api_key: &str) -> Result<(), String> {
        minimax_storage::write_api_key(account_id, api_key)
    }

    fn save_error_prefix() -> &'static str {
        "Failed to save Minimax API key:"
    }
}

pub(super) struct OpenCodeGoCase;

impl KeyAuthenticationCase for OpenCodeGoCase {
    type Flow = super::super::OpenCodeGoLoginFlow;

    const PROVIDER: ProviderId = ProviderId::OpenCodeGo;

    fn state(app: &AppModel) -> Option<&KeyAuthenticationState> {
        app.opencode_go_login.as_ref()
    }

    fn opencode_auth(key: &str) -> String {
        format!(r#"{{"opencode-go":{{"type":"api","key":"{key}"}}}}"#)
    }

    fn storage_root(root: &Path) -> PathBuf {
        root.join("yapcap/opencode-go-accounts")
    }

    fn account_facts(config: &Config, account_id: &str) -> Option<AccountFacts> {
        config
            .opencode_go_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(AccountFacts::from_opencode_go)
    }

    fn set_account(config: &mut Config, account_id: &str, label: &str, created_at: DateTime<Utc>) {
        config
            .opencode_go_managed_accounts
            .push(ManagedOpenCodeGoAccountConfig {
                id: account_id.to_string(),
                label: label.to_string(),
                api_key_source: "stored".to_string(),
                created_at,
                updated_at: created_at,
                last_authenticated_at: Some(created_at),
            });
    }

    fn load_api_key(account_id: &str) -> Result<String, String> {
        opencode_go_storage::load_api_key(account_id)
    }

    fn write_api_key(account_id: &str, api_key: &str) -> Result<(), String> {
        opencode_go_storage::write_api_key(account_id, api_key)
    }

    fn save_error_prefix() -> &'static str {
        "Failed to save OpenCode Go API key:"
    }
}

pub(super) struct ZaiCase;

impl KeyAuthenticationCase for ZaiCase {
    type Flow = super::super::ZaiLoginFlow;

    const PROVIDER: ProviderId = ProviderId::Zai;

    fn state(app: &AppModel) -> Option<&KeyAuthenticationState> {
        app.zai_login.as_ref()
    }

    fn opencode_auth(key: &str) -> String {
        format!(r#"{{"zai-coding-plan":{{"type":"api","key":"{key}"}}}}"#)
    }

    fn storage_root(root: &Path) -> PathBuf {
        root.join("yapcap/zai-accounts")
    }

    fn account_facts(config: &Config, account_id: &str) -> Option<AccountFacts> {
        config
            .zai_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(AccountFacts::from_zai)
    }

    fn set_account(config: &mut Config, account_id: &str, label: &str, created_at: DateTime<Utc>) {
        config.zai_managed_accounts.push(ManagedZaiAccountConfig {
            id: account_id.to_string(),
            label: label.to_string(),
            api_key_source: "stored".to_string(),
            created_at,
            updated_at: created_at,
            last_authenticated_at: Some(created_at),
        });
    }

    fn load_api_key(account_id: &str) -> Result<String, String> {
        zai_storage::load_api_key(account_id)
    }

    fn write_api_key(account_id: &str, api_key: &str) -> Result<(), String> {
        zai_storage::write_api_key(account_id, api_key)
    }

    fn save_error_prefix() -> &'static str {
        "Failed to save Z.AI API key:"
    }
}

#[derive(Debug)]
pub(super) struct AccountFacts {
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

impl AccountFacts {
    fn from_kimi(account: &ManagedKimiAccountConfig) -> Self {
        Self {
            label: account.label.clone(),
            created_at: account.created_at,
            updated_at: account.updated_at,
            last_authenticated_at: account.last_authenticated_at,
        }
    }

    fn from_openrouter(account: &ManagedOpenRouterAccountConfig) -> Self {
        Self {
            label: account.label.clone(),
            created_at: account.created_at,
            updated_at: account.updated_at,
            last_authenticated_at: account.last_authenticated_at,
        }
    }

    fn from_minimax(account: &ManagedMinimaxAccountConfig) -> Self {
        Self {
            label: account.label.clone(),
            created_at: account.created_at,
            updated_at: account.updated_at,
            last_authenticated_at: account.last_authenticated_at,
        }
    }

    fn from_opencode_go(account: &ManagedOpenCodeGoAccountConfig) -> Self {
        Self {
            label: account.label.clone(),
            created_at: account.created_at,
            updated_at: account.updated_at,
            last_authenticated_at: account.last_authenticated_at,
        }
    }

    fn from_zai(account: &ManagedZaiAccountConfig) -> Self {
        Self {
            label: account.label.clone(),
            created_at: account.created_at,
            updated_at: account.updated_at,
            last_authenticated_at: account.last_authenticated_at,
        }
    }
}
