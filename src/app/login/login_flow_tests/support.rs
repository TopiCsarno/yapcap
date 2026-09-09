use crate::app::AppModel;
use crate::config::{
    Config, ManagedAntigravityAccountConfig, ManagedClaudeAccountConfig, ManagedCodexAccountConfig,
    ManagedCopilotAccountConfig, ManagedGeminiAccountConfig, ManagedGrokAccountConfig,
};
use crate::model::ProviderId;
use crate::providers::cursor::CursorScanState;
use chrono::Utc;
use std::path::PathBuf;

pub(super) fn test_app() -> AppModel {
    let lock_path = std::env::temp_dir().join("yapcap-login-flow-test-unused-owner.lock");
    AppModel {
        core: cosmic::Core::default(),
        popup: None,
        config: Config::default(),
        state: crate::model::AppState::empty(),
        detection: crate::detection::DetectionSnapshot::default(),
        selected_provider: ProviderId::Codex,
        detail_account_page: 0,
        provider_viewport_offset: 0,
        popup_route: crate::app::PopupRoute::ProviderDetail,
        update_status: crate::updates::UpdateStatus::Unchecked,
        launch_mode: crate::app::LaunchMode::Standalone,
        shared_control: Default::default(),
        process_info: crate::refresh_owner::ProcessInfo {
            id: "login-flow-test-process".to_string(),
            pid: std::process::id(),
            panel_output: None,
            flatpak_id: None,
            lock_path,
        },
        refresh_owner: None,
        codex_login: None,
        codex_login_handle: None,
        claude_login: None,
        claude_login_handle: None,
        cursor_scan: CursorScanState::Idle,
        cursor_scan_result: None,
        gemini_login: None,
        gemini_login_handle: None,
        copilot_login: None,
        copilot_login_handle: None,
        minimax_login: None,
        minimax_login_handle: None,
        kimi_login: None,
        kimi_login_handle: None,
        antigravity_login: None,
        antigravity_login_handle: None,
        opencode_go_login: None,
        opencode_go_login_handle: None,
        grok_login: None,
        grok_login_handle: None,
    }
}

pub(super) fn isolated_xdg(name: &str) -> (crate::test_support::TestEnv, PathBuf) {
    let mut env = crate::test_support::test_env();
    let state_root = std::env::temp_dir().join(format!(
        "yapcap-login-flow-test-{name}-{}",
        std::process::id()
    ));
    env.set("XDG_STATE_HOME", &state_root);
    env.set("XDG_CONFIG_HOME", &state_root);
    (env, state_root)
}

pub(super) fn codex_account(id: &str) -> ManagedCodexAccountConfig {
    ManagedCodexAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        codex_home: PathBuf::from("/tmp/yapcap/codex-1"),
        email: Some("user@example.com".to_string()),
        provider_account_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

pub(super) fn claude_account(id: &str) -> ManagedClaudeAccountConfig {
    ManagedClaudeAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        config_dir: PathBuf::from("/tmp/yapcap/claude"),
        email: Some(format!("{id}@example.com")),
        organization: None,
        subscription_type: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

pub(super) fn gemini_account(id: &str) -> ManagedGeminiAccountConfig {
    ManagedGeminiAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        account_root: PathBuf::from("/tmp/yapcap/gemini"),
        email: format!("{id}@example.com"),
        sub: id.to_string(),
        hd: None,
        last_tier_id: None,
        last_cloudaicompanion_project: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

pub(super) fn antigravity_account(id: &str) -> ManagedAntigravityAccountConfig {
    ManagedAntigravityAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        account_root: PathBuf::from("/tmp/yapcap/antigravity"),
        email: format!("{id}@example.com"),
        sub: id.to_string(),
        last_tier_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: None,
    }
}

pub(super) fn copilot_account(id: &str, login: &str) -> ManagedCopilotAccountConfig {
    ManagedCopilotAccountConfig {
        id: id.to_string(),
        label: login.to_string(),
        github_user_id: 1,
        login: login.to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: Some(Utc::now()),
    }
}

pub(super) fn grok_account(id: &str) -> ManagedGrokAccountConfig {
    ManagedGrokAccountConfig {
        id: id.to_string(),
        label: id.to_string(),
        config_dir: PathBuf::from(format!("/tmp/yapcap/grok/{id}")),
        email: Some(format!("{id}@example.com")),
        provider_account_id: Some(format!("uid-{id}")),
        team_id: None,
        plan: Some("SuperGrok".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_authenticated_at: Some(Utc::now()),
    }
}
