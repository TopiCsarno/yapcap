// SPDX-License-Identifier: MPL-2.0

use crate::config::{Config, ManagedOpenRouterAccountConfig};
use crate::key_authentication::{
    KeyAuthenticationAdapter, KeyAuthenticationTarget, prepare as prepare_key_authentication,
    prepare_for_reauth as prepare_key_authentication_for_reauth,
};
use crate::providers::openrouter::{opencode, storage};
use chrono::{DateTime, Utc};

pub use crate::key_authentication::{
    KeyAuthenticationEvent as OpenRouterLoginEvent, KeyAuthenticationState as OpenRouterLoginState,
    KeyAuthenticationStatus as OpenRouterLoginStatus,
};

pub(crate) struct OpenRouterKeyAuthentication;

impl KeyAuthenticationAdapter for OpenRouterKeyAuthentication {
    type Account = ManagedOpenRouterAccountConfig;

    fn new_account_id() -> String {
        format!(
            "openrouter-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )
    }

    fn find_account(config: &Config, account_id: &str) -> Result<KeyAuthenticationTarget, String> {
        config
            .openrouter_managed_accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| KeyAuthenticationTarget {
                id: account.id.clone(),
                label: account.label.clone(),
                created_at: account.created_at,
            })
            .ok_or_else(|| "OpenRouter account not found".to_string())
    }

    fn discover_api_key() -> Option<String> {
        opencode::discover_api_key()
    }

    fn empty_key_error() -> String {
        "API key is required".to_string()
    }

    fn validate(
        config: &Config,
        account_id: &str,
        label: &str,
        api_key: &str,
    ) -> Result<(), String> {
        if label.trim().is_empty() {
            return Err("Account name is required".to_string());
        }
        if config
            .openrouter_managed_accounts
            .iter()
            .any(|account| account.id != account_id && account.label == label)
        {
            return Err("An account with this name already exists".to_string());
        }
        if config.openrouter_managed_accounts.iter().any(|account| {
            account.id != account_id
                && storage::load_api_key(&account.id)
                    .ok()
                    .is_some_and(|stored_key| stored_key == api_key)
        }) {
            return Err("An account with this API key already exists".to_string());
        }
        Ok(())
    }

    fn build_account(
        account_id: String,
        label: String,
        created_at: DateTime<Utc>,
        authenticated_at: DateTime<Utc>,
    ) -> Self::Account {
        ManagedOpenRouterAccountConfig {
            id: account_id,
            label,
            api_key_source: "stored".to_string(),
            created_at,
            updated_at: authenticated_at,
            last_authenticated_at: Some(authenticated_at),
        }
    }

    fn persist(account: &Self::Account, api_key: &str) -> Result<(), String> {
        storage::write_api_key(&account.id, api_key)
            .map_err(|error| format!("Failed to save OpenRouter API key: {error}"))
    }
}

pub fn prepare() -> OpenRouterLoginState {
    prepare_key_authentication::<OpenRouterKeyAuthentication>()
}

pub fn prepare_for_reauth(
    config: Config,
    account_id: &str,
) -> Result<OpenRouterLoginState, String> {
    prepare_key_authentication_for_reauth::<OpenRouterKeyAuthentication>(&config, account_id)
}

pub(crate) fn save(
    config: &Config,
    state: &mut OpenRouterLoginState,
) -> Result<ManagedOpenRouterAccountConfig, String> {
    state.save::<OpenRouterKeyAuthentication>(config)
}
