use super::key_authentication_cases::{
    KeyAuthenticationCase, KimiCase, MinimaxCase, OpenCodeGoCase, OpenRouterCase, ZaiCase,
};
use super::support::{isolated_xdg, test_app};
use crate::app::AppModel;
use crate::app::login::{LoginFlow, reauthenticate, start_login};
use crate::key_authentication::{KeyAuthenticationEvent, KeyAuthenticationStatus};
use crate::shared_state::RefreshRequestReason;
use chrono::{Duration, Utc};
use std::path::PathBuf;

fn setup<C: KeyAuthenticationCase>(
    name: &str,
) -> (crate::test_support::TestEnv, PathBuf, AppModel) {
    let (mut env, root) = isolated_xdg(name);
    env.remove(crate::providers::opencode_auth::OPENCODE_AUTH_CONTENT_ENV);
    env.set(
        crate::providers::opencode_auth::OPENCODE_AUTH_PATH_ENV,
        root.join("missing-auth.json"),
    );
    (env, root, test_app())
}

fn send<C: KeyAuthenticationCase>(app: &mut AppModel, event: KeyAuthenticationEvent) {
    let _ = C::Flow::on_event(app, event);
}

fn start<C: KeyAuthenticationCase>(app: &mut AppModel) {
    let _ = start_login::<C::Flow>(app);
}

fn assert_empty_key_is_editable<C: KeyAuthenticationCase>(name: &str) {
    let (_env, _root, mut app) = setup::<C>(name);
    start::<C>(&mut app);

    send::<C>(&mut app, KeyAuthenticationEvent::Saved);

    let login = C::state(&app).expect("key form should remain open");
    assert_eq!(login.status, KeyAuthenticationStatus::Editing);
    assert_eq!(login.error.as_deref(), Some("API key is required"));
    assert!(app.config.selected_account_ids(C::PROVIDER).is_empty());
    assert!(app.shared_control.requests.is_empty());
}

fn assert_masking_and_visibility_are_shared<C: KeyAuthenticationCase>(name: &str) {
    let (_env, _root, mut app) = setup::<C>(name);
    start::<C>(&mut app);

    let login = C::state(&app).unwrap();
    assert!(login.api_key.is_empty());
    assert!(!login.api_key_visible);
    assert!(!login.api_key_from_opencode);

    send::<C>(&mut app, KeyAuthenticationEvent::ApiKeyVisibilityToggled);
    assert!(C::state(&app).unwrap().api_key_visible);
    send::<C>(&mut app, KeyAuthenticationEvent::ApiKeyVisibilityToggled);
    assert!(!C::state(&app).unwrap().api_key_visible);
}

fn assert_imported_provenance_is_cleared_by_input<C: KeyAuthenticationCase>(name: &str) {
    let (mut env, root, mut app) = setup::<C>(name);
    let auth_path = root.join("auth.json");
    std::fs::write(&auth_path, C::opencode_auth("imported-key")).unwrap();
    env.set(
        crate::providers::opencode_auth::OPENCODE_AUTH_PATH_ENV,
        &auth_path,
    );

    start::<C>(&mut app);

    let login = C::state(&app).unwrap();
    assert_eq!(login.api_key, "imported-key");
    assert!(login.api_key_from_opencode);
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::ApiKeyChanged("typed-key".to_string()),
    );
    assert_eq!(C::state(&app).unwrap().api_key, "typed-key");
    assert!(!C::state(&app).unwrap().api_key_from_opencode);
}

fn assert_save_failure_is_editable<C: KeyAuthenticationCase>(name: &str) {
    let (_env, root, mut app) = setup::<C>(name);
    start::<C>(&mut app);
    let account_id = C::state(&app).unwrap().account_id.clone();
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::LabelChanged("New account".to_string()),
    );
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::ApiKeyChanged("typed-key".to_string()),
    );

    let accounts_root = C::storage_root(&root);
    let outside = root.join("outside");
    std::fs::create_dir_all(&accounts_root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, accounts_root.join(&account_id)).unwrap();

    send::<C>(&mut app, KeyAuthenticationEvent::Saved);

    let login = C::state(&app).expect("failed save should keep the form");
    assert_eq!(login.status, KeyAuthenticationStatus::Editing);
    assert_eq!(login.api_key, "typed-key");
    assert!(
        login
            .error
            .as_deref()
            .is_some_and(|error| { error.starts_with(C::save_error_prefix()) })
    );
    assert!(app.config.selected_account_ids(C::PROVIDER).is_empty());
}

fn assert_save_selects_and_refreshes<C: KeyAuthenticationCase>(name: &str) {
    let (_env, _root, mut app) = setup::<C>(name);
    start::<C>(&mut app);
    let account_id = C::state(&app).unwrap().account_id.clone();
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::LabelChanged("New account".to_string()),
    );
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::ApiKeyChanged("new-key".to_string()),
    );

    let task = C::Flow::on_event(&mut app, KeyAuthenticationEvent::Saved);

    assert_eq!(task.units(), 0);
    assert!(C::state(&app).is_none());
    assert_eq!(
        app.config.selected_account_ids(C::PROVIDER),
        std::slice::from_ref(&account_id)
    );
    assert_eq!(C::load_api_key(&account_id).unwrap(), "new-key");
    assert!(
        app.state
            .accounts_for(C::PROVIDER)
            .iter()
            .any(|account| account.account_id == account_id)
    );
    assert_eq!(app.shared_control.requests.len(), 1);
    assert_eq!(
        app.shared_control.requests[0].reason,
        RefreshRequestReason::AccountAction
    );
}

fn assert_reauthentication_preserves_identity<C: KeyAuthenticationCase>(name: &str) {
    let (_env, _root, mut app) = setup::<C>(name);
    let account_id = "existing-account";
    let created_at = Utc::now() - Duration::days(2);
    let updated_at = Utc::now() - Duration::days(1);
    C::set_account(&mut app.config, account_id, "Existing", created_at);
    C::write_api_key(account_id, "old-key").unwrap();

    let _ = reauthenticate::<C::Flow>(&mut app, account_id);

    let login = C::state(&app).expect("reauthentication should start");
    assert_eq!(login.account_id, account_id);
    assert_eq!(login.label, "Existing");
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::LabelChanged("Changed label".to_string()),
    );
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::ApiKeyChanged("new-key".to_string()),
    );
    let _ = C::Flow::on_event(&mut app, KeyAuthenticationEvent::Saved);

    let facts = C::account_facts(&app.config, account_id).unwrap();
    assert_eq!(facts.label, "Existing");
    assert_eq!(facts.created_at, created_at);
    assert!(facts.updated_at > updated_at);
    assert!(
        facts
            .last_authenticated_at
            .is_some_and(|authenticated_at| authenticated_at > updated_at)
    );
    assert_eq!(C::load_api_key(account_id).unwrap(), "new-key");
    assert!(C::state(&app).is_none());
}

fn assert_rejects_invalid_account_details<C: KeyAuthenticationCase>(name: &str) {
    let (_env, _root, mut app) = setup::<C>(name);
    start::<C>(&mut app);
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::LabelChanged("  ".to_string()),
    );
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::ApiKeyChanged("new-key".to_string()),
    );
    send::<C>(&mut app, KeyAuthenticationEvent::Saved);

    let login = C::state(&app).expect("invalid account should remain editable");
    assert_eq!(login.error.as_deref(), Some("Account name is required"));
    assert!(app.config.selected_account_ids(C::PROVIDER).is_empty());
}

fn assert_rejects_duplicate_account_details<C: KeyAuthenticationCase>(name: &str) {
    let (_env, _root, mut app) = setup::<C>(name);
    let existing_id = "existing-account";
    C::set_account(&mut app.config, existing_id, "Existing", Utc::now());
    C::write_api_key(existing_id, "existing-key").unwrap();

    start::<C>(&mut app);
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::LabelChanged("Existing".to_string()),
    );
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::ApiKeyChanged("new-key".to_string()),
    );
    send::<C>(&mut app, KeyAuthenticationEvent::Saved);

    let login = C::state(&app).expect("duplicate name should remain editable");
    assert_eq!(
        login.error.as_deref(),
        Some("An account with this name already exists")
    );

    send::<C>(
        &mut app,
        KeyAuthenticationEvent::LabelChanged("New account".to_string()),
    );
    send::<C>(
        &mut app,
        KeyAuthenticationEvent::ApiKeyChanged("existing-key".to_string()),
    );
    send::<C>(&mut app, KeyAuthenticationEvent::Saved);

    let login = C::state(&app).expect("duplicate key should remain editable");
    assert_eq!(
        login.error.as_deref(),
        Some("An account with this API key already exists")
    );
    assert!(app.config.selected_account_ids(C::PROVIDER).is_empty());
}

#[test]
fn key_authentication_rejects_empty_keys_for_all_api_key_providers() {
    assert_empty_key_is_editable::<KimiCase>("common-empty-kimi");
    assert_empty_key_is_editable::<OpenRouterCase>("common-empty-openrouter");
    assert_empty_key_is_editable::<MinimaxCase>("common-empty-minimax");
    assert_empty_key_is_editable::<OpenCodeGoCase>("common-empty-opencode-go");
    assert_empty_key_is_editable::<ZaiCase>("common-empty-zai");
}

#[test]
fn key_authentication_shares_masking_and_visibility_for_all_api_key_providers() {
    assert_masking_and_visibility_are_shared::<KimiCase>("common-visibility-kimi");
    assert_masking_and_visibility_are_shared::<OpenRouterCase>("common-visibility-openrouter");
    assert_masking_and_visibility_are_shared::<MinimaxCase>("common-visibility-minimax");
    assert_masking_and_visibility_are_shared::<OpenCodeGoCase>("common-visibility-opencode-go");
    assert_masking_and_visibility_are_shared::<ZaiCase>("common-visibility-zai");
}

#[test]
fn key_authentication_shares_import_provenance_for_all_api_key_providers() {
    assert_imported_provenance_is_cleared_by_input::<KimiCase>("common-provenance-kimi");
    assert_imported_provenance_is_cleared_by_input::<OpenRouterCase>(
        "common-provenance-openrouter",
    );
    assert_imported_provenance_is_cleared_by_input::<MinimaxCase>("common-provenance-minimax");
    assert_imported_provenance_is_cleared_by_input::<OpenCodeGoCase>(
        "common-provenance-opencode-go",
    );
    assert_imported_provenance_is_cleared_by_input::<ZaiCase>("common-provenance-zai");
}

#[cfg(unix)]
#[test]
fn key_authentication_preserves_editable_forms_after_save_failure_for_all_providers() {
    assert_save_failure_is_editable::<KimiCase>("common-failure-kimi");
    assert_save_failure_is_editable::<OpenRouterCase>("common-failure-openrouter");
    assert_save_failure_is_editable::<MinimaxCase>("common-failure-minimax");
    assert_save_failure_is_editable::<OpenCodeGoCase>("common-failure-opencode-go");
    assert_save_failure_is_editable::<ZaiCase>("common-failure-zai");
}

#[test]
fn key_authentication_success_selects_and_requests_refresh_for_all_providers() {
    assert_save_selects_and_refreshes::<KimiCase>("common-success-kimi");
    assert_save_selects_and_refreshes::<OpenRouterCase>("common-success-openrouter");
    assert_save_selects_and_refreshes::<MinimaxCase>("common-success-minimax");
    assert_save_selects_and_refreshes::<OpenCodeGoCase>("common-success-opencode-go");
    assert_save_selects_and_refreshes::<ZaiCase>("common-success-zai");
}

#[test]
fn key_authentication_reauthentication_preserves_identity_for_all_providers() {
    assert_reauthentication_preserves_identity::<KimiCase>("common-reauth-kimi");
    assert_reauthentication_preserves_identity::<OpenRouterCase>("common-reauth-openrouter");
    assert_reauthentication_preserves_identity::<MinimaxCase>("common-reauth-minimax");
    assert_reauthentication_preserves_identity::<OpenCodeGoCase>("common-reauth-opencode-go");
    assert_reauthentication_preserves_identity::<ZaiCase>("common-reauth-zai");
}

#[test]
fn key_authentication_rejects_empty_account_names_for_all_providers() {
    assert_rejects_invalid_account_details::<KimiCase>("common-empty-name-kimi");
    assert_rejects_invalid_account_details::<OpenRouterCase>("common-empty-name-openrouter");
    assert_rejects_invalid_account_details::<MinimaxCase>("common-empty-name-minimax");
    assert_rejects_invalid_account_details::<OpenCodeGoCase>("common-empty-name-opencode-go");
    assert_rejects_invalid_account_details::<ZaiCase>("common-empty-name-zai");
}

#[test]
fn key_authentication_rejects_duplicate_names_and_keys_for_all_providers() {
    assert_rejects_duplicate_account_details::<KimiCase>("common-duplicate-kimi");
    assert_rejects_duplicate_account_details::<OpenRouterCase>("common-duplicate-openrouter");
    assert_rejects_duplicate_account_details::<MinimaxCase>("common-duplicate-minimax");
    assert_rejects_duplicate_account_details::<OpenCodeGoCase>("common-duplicate-opencode-go");
    assert_rejects_duplicate_account_details::<ZaiCase>("common-duplicate-zai");
}
