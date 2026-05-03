use super::*;
use crate::config::Config;
use crate::config::ProviderVisibilityMode;

#[test]
fn providers_expose_expected_capabilities() {
    assert_eq!(
        capabilities(ProviderId::Codex),
        ProviderCapabilities {
            supports_delete: true,
            supports_reauthentication: false,
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    );
    assert_eq!(
        capabilities(ProviderId::Claude),
        ProviderCapabilities {
            supports_delete: true,
            supports_reauthentication: true,
            supports_background_status_refresh: false,
            requires_auth_prompt_on_auth_failure: false,
        }
    );
    assert_eq!(
        capabilities(ProviderId::Cursor),
        ProviderCapabilities {
            supports_delete: true,
            supports_reauthentication: true,
            supports_background_status_refresh: true,
            requires_auth_prompt_on_auth_failure: true,
        }
    );
}

#[test]
fn cursor_supports_background_status_refresh() {
    assert!(supports_background_status_refresh(ProviderId::Cursor));
    assert!(!supports_background_status_refresh(ProviderId::Codex));
    assert!(!supports_background_status_refresh(ProviderId::Claude));
}

#[test]
fn cursor_requires_reauth_prompt_on_auth_error() {
    assert!(auth_error_requires_reauth_prompt(ProviderId::Cursor));
    assert!(!auth_error_requires_reauth_prompt(ProviderId::Codex));
    assert!(!auth_error_requires_reauth_prompt(ProviderId::Claude));
}

#[test]
fn each_provider_resolves_accounts() {
    let config = Config::default();
    for provider in ProviderId::ALL {
        let accounts = discover_accounts(provider, &config);
        assert!(
            accounts.is_empty(),
            "default config should have no accounts for {provider:?}"
        );
    }
}

#[test]
fn initialize_provider_visibility_enables_provider_regardless_of_accounts() {
    let mut config = Config {
        cursor_enabled: false,
        ..Config::default()
    };
    assert!(initialize_provider_visibility(
        &mut config,
        &[ProviderId::Cursor]
    ));
    assert!(config.cursor_enabled);
    assert_eq!(
        config.provider_visibility_mode,
        ProviderVisibilityMode::AutoInitPending
    );
}

#[test]
fn initialize_provider_visibility_is_noop_after_initialization() {
    let mut config = Config {
        provider_visibility_mode: ProviderVisibilityMode::UserManaged,
        ..Config::default()
    };

    assert!(!initialize_provider_visibility(
        &mut config,
        &[ProviderId::Codex, ProviderId::Claude, ProviderId::Cursor]
    ));
    assert!(config.codex_enabled);
    assert!(config.claude_enabled);
    assert!(config.cursor_enabled);
}

#[test]
fn action_support_matches_capabilities() {
    let support = capabilities(ProviderId::Cursor).action_support();
    assert!(support.can_delete);
    assert!(support.can_reauthenticate);
    assert!(support.supports_background_status_refresh);
}
