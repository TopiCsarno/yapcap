// SPDX-License-Identifier: MPL-2.0

use super::support::{isolated_xdg, test_app};
use crate::app::login::{OpenRouterLoginFlow, reauthenticate, start_login};
use crate::config::Config;
use crate::providers::openrouter::login::prepare_for_reauth;
use std::fs;

#[test]
fn openrouter_login_uses_the_openrouter_opencode_credential_for_prefill() {
    let (mut env, root) = isolated_xdg("openrouter-opencode-prefill");
    fs::create_dir_all(&root).unwrap();
    let auth_path = root.join("auth.json");
    fs::write(
        &auth_path,
        r#"{"openrouter":{"type":"api","key":"fake-openrouter-key"}}"#,
    )
    .unwrap();
    env.set("YAPCAP_OPENCODE_AUTH_PATH", &auth_path);
    let mut app = test_app();

    let _ = start_login::<OpenRouterLoginFlow>(&mut app);

    let login = app.openrouter_login.as_ref().unwrap();
    assert_eq!(login.api_key, "fake-openrouter-key");
    assert!(login.api_key_from_opencode);
}

#[test]
fn openrouter_reauth_reports_an_openrouter_specific_missing_account_error() {
    let (_env, _root) = isolated_xdg("openrouter-missing-account");

    assert_eq!(
        prepare_for_reauth(Config::default(), "missing").unwrap_err(),
        "OpenRouter account not found"
    );
}

#[test]
fn openrouter_reauth_is_not_started_for_an_unknown_account() {
    let (_env, _root) = isolated_xdg("openrouter-unknown-reauth");
    let mut app = test_app();

    let _ = reauthenticate::<OpenRouterLoginFlow>(&mut app, "missing");

    assert!(app.openrouter_login.is_none());
}
