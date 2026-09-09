// SPDX-License-Identifier: MPL-2.0

use super::Config;

impl Config {
    pub fn apply_watcher_update(&mut self, update: Self, keys: &[&str]) {
        if keys.is_empty() {
            *self = update;
            return;
        }
        for key in keys {
            if !self.apply_general_watcher_key(&update, key) {
                self.apply_account_watcher_key(&update, key);
            }
        }
    }

    fn apply_general_watcher_key(&mut self, update: &Self, key: &str) -> bool {
        match key {
            "refresh_interval_seconds" => {
                self.refresh_interval_seconds = update.refresh_interval_seconds;
            }
            "reset_time_format" => self.reset_time_format = update.reset_time_format,
            "usage_amount_format" => self.usage_amount_format = update.usage_amount_format,
            "panel_icon_style" => self.panel_icon_style = update.panel_icon_style,
            "selected_provider" => self.selected_provider = update.selected_provider,
            "provider_visibility_mode" => {
                self.provider_visibility_mode = update.provider_visibility_mode;
            }
            "codex_enablement" => self.codex_enablement = update.codex_enablement,
            "claude_enablement" => self.claude_enablement = update.claude_enablement,
            "cursor_enablement" => self.cursor_enablement = update.cursor_enablement,
            "gemini_enablement" => self.gemini_enablement = update.gemini_enablement,
            "copilot_enablement" => self.copilot_enablement = update.copilot_enablement,
            "minimax_enablement" => self.minimax_enablement = update.minimax_enablement,
            "kimi_enablement" => self.kimi_enablement = update.kimi_enablement,
            "antigravity_enablement" => {
                self.antigravity_enablement = update.antigravity_enablement;
            }
            "opencode_go_enablement" => {
                self.opencode_go_enablement = update.opencode_go_enablement;
            }
            "grok_enablement" => {
                self.grok_enablement = update.grok_enablement;
            }
            "log_level" => self.log_level.clone_from(&update.log_level),
            _ => return false,
        }
        true
    }

    fn apply_account_watcher_key(&mut self, update: &Self, key: &str) {
        match key {
            "selected_codex_account_ids" => {
                self.selected_codex_account_ids = update.selected_codex_account_ids.clone();
            }
            "codex_managed_accounts" => {
                self.codex_managed_accounts = update.codex_managed_accounts.clone();
            }
            "selected_claude_account_ids" => {
                self.selected_claude_account_ids = update.selected_claude_account_ids.clone();
            }
            "claude_managed_accounts" => {
                self.claude_managed_accounts = update.claude_managed_accounts.clone();
            }
            "selected_cursor_account_ids" => {
                self.selected_cursor_account_ids = update.selected_cursor_account_ids.clone();
            }
            "cursor_managed_accounts" => {
                self.cursor_managed_accounts = update.cursor_managed_accounts.clone();
            }
            "selected_gemini_account_ids" => {
                self.selected_gemini_account_ids = update.selected_gemini_account_ids.clone();
            }
            "gemini_managed_accounts" => {
                self.gemini_managed_accounts = update.gemini_managed_accounts.clone();
            }
            "selected_copilot_account_ids" => {
                self.selected_copilot_account_ids = update.selected_copilot_account_ids.clone();
            }
            "copilot_managed_accounts" => {
                self.copilot_managed_accounts = update.copilot_managed_accounts.clone();
            }
            "selected_minimax_account_ids" => {
                self.selected_minimax_account_ids = update.selected_minimax_account_ids.clone();
            }
            "minimax_managed_accounts" => {
                self.minimax_managed_accounts = update.minimax_managed_accounts.clone();
            }
            "selected_kimi_account_ids" => {
                self.selected_kimi_account_ids = update.selected_kimi_account_ids.clone();
            }
            "kimi_managed_accounts" => {
                self.kimi_managed_accounts = update.kimi_managed_accounts.clone();
            }
            "selected_antigravity_account_ids" => {
                self.selected_antigravity_account_ids =
                    update.selected_antigravity_account_ids.clone();
            }
            "antigravity_managed_accounts" => {
                self.antigravity_managed_accounts = update.antigravity_managed_accounts.clone();
            }
            "selected_opencode_go_account_ids" => {
                self.selected_opencode_go_account_ids =
                    update.selected_opencode_go_account_ids.clone();
            }
            "opencode_go_managed_accounts" => {
                self.opencode_go_managed_accounts = update.opencode_go_managed_accounts.clone();
            }
            "selected_grok_account_ids" => {
                self.selected_grok_account_ids = update.selected_grok_account_ids.clone();
            }
            "grok_managed_accounts" => {
                self.grok_managed_accounts = update.grok_managed_accounts.clone();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ManagedGrokAccountConfig, ManagedKimiAccountConfig, ManagedMinimaxAccountConfig,
        ManagedOpenCodeGoAccountConfig, ProviderEnablement,
    };
    use chrono::Utc;

    #[test]
    fn applies_minimax_watcher_keys_without_replacing_unrelated_configuration() {
        let mut config = Config::default();
        let mut update = Config {
            codex_enablement: crate::config::ProviderEnablement::Disabled,
            minimax_enablement: crate::config::ProviderEnablement::Disabled,
            selected_minimax_account_ids: vec!["minimax-2".to_string()],
            minimax_managed_accounts: vec![minimax_account("minimax-2")],
            ..Config::default()
        };
        update.minimax_managed_accounts[0].label = "Second Minimax".to_string();

        config.apply_watcher_update(
            update,
            &[
                "minimax_enablement",
                "selected_minimax_account_ids",
                "minimax_managed_accounts",
            ],
        );

        assert_eq!(
            config.minimax_enablement,
            crate::config::ProviderEnablement::Disabled
        );
        assert_eq!(config.selected_minimax_account_ids, ["minimax-2"]);
        assert_eq!(config.minimax_managed_accounts[0].label, "Second Minimax");
        assert_eq!(
            config.codex_enablement,
            crate::config::ProviderEnablement::Auto
        );
    }

    #[test]
    fn applies_kimi_watcher_keys_without_replacing_unrelated_configuration() {
        let mut config = Config::default();
        let mut update = Config {
            codex_enablement: crate::config::ProviderEnablement::Disabled,
            kimi_enablement: crate::config::ProviderEnablement::Disabled,
            selected_kimi_account_ids: vec!["kimi-2".to_string()],
            kimi_managed_accounts: vec![kimi_account("kimi-2")],
            ..Config::default()
        };
        update.kimi_managed_accounts[0].label = "Second Kimi".to_string();

        config.apply_watcher_update(
            update,
            &[
                "kimi_enablement",
                "selected_kimi_account_ids",
                "kimi_managed_accounts",
            ],
        );

        assert_eq!(
            config.kimi_enablement,
            crate::config::ProviderEnablement::Disabled
        );
        assert_eq!(config.selected_kimi_account_ids, ["kimi-2"]);
        assert_eq!(config.kimi_managed_accounts[0].label, "Second Kimi");
        assert_eq!(
            config.codex_enablement,
            crate::config::ProviderEnablement::Auto
        );
    }

    #[test]
    fn applies_opencode_go_watcher_keys_without_replacing_unrelated_configuration() {
        let mut config = Config::default();
        let now = Utc::now();
        let update = Config {
            codex_enablement: ProviderEnablement::Disabled,
            opencode_go_enablement: ProviderEnablement::Enabled,
            selected_opencode_go_account_ids: vec!["go-1".to_string()],
            opencode_go_managed_accounts: vec![ManagedOpenCodeGoAccountConfig {
                id: "go-1".to_string(),
                label: "OpenCode Go".to_string(),
                api_key_source: "stored".to_string(),
                created_at: now,
                updated_at: now,
                last_authenticated_at: None,
            }],
            ..Config::default()
        };

        config.apply_watcher_update(
            update,
            &[
                "opencode_go_enablement",
                "selected_opencode_go_account_ids",
                "opencode_go_managed_accounts",
            ],
        );

        assert_eq!(config.opencode_go_enablement, ProviderEnablement::Enabled);
        assert_eq!(config.selected_opencode_go_account_ids, ["go-1"]);
        assert_eq!(config.opencode_go_managed_accounts[0].id, "go-1");
        assert_eq!(config.codex_enablement, ProviderEnablement::Auto);
    }

    #[test]
    fn applies_grok_watcher_keys_without_replacing_unrelated_configuration() {
        let mut config = Config::default();
        let now = Utc::now();
        let update = Config {
            grok_enablement: ProviderEnablement::Enabled,
            selected_grok_account_ids: vec!["grok-1".to_string()],
            grok_managed_accounts: vec![ManagedGrokAccountConfig {
                id: "grok-1".to_string(),
                label: "Grok User".to_string(),
                config_dir: std::path::PathBuf::from("/tmp/grok-1"),
                email: Some("grok@example.com".to_string()),
                provider_account_id: Some("user-grok".to_string()),
                team_id: None,
                plan: Some("SuperGrok".to_string()),
                created_at: now,
                updated_at: now,
                last_authenticated_at: None,
            }],
            ..Config::default()
        };

        config.apply_watcher_update(
            update,
            &[
                "grok_enablement",
                "selected_grok_account_ids",
                "grok_managed_accounts",
            ],
        );

        assert_eq!(config.grok_enablement, ProviderEnablement::Enabled);
        assert_eq!(config.selected_grok_account_ids, ["grok-1"]);
        assert_eq!(config.grok_managed_accounts[0].id, "grok-1");
        assert_eq!(config.codex_enablement, ProviderEnablement::Auto);
    }

    #[test]
    fn ignores_unknown_keys() {
        let mut config = Config::default();
        let update = Config {
            kimi_enablement: crate::config::ProviderEnablement::Disabled,
            ..Config::default()
        };

        config.apply_watcher_update(update, &["unknown"]);

        assert_eq!(
            config.kimi_enablement,
            crate::config::ProviderEnablement::Auto
        );
    }

    #[test]
    fn empty_keys_replace_the_entire_configuration() {
        let mut config = Config::default();
        let update = Config {
            kimi_enablement: crate::config::ProviderEnablement::Disabled,
            ..Config::default()
        };

        config.apply_watcher_update(update, &[]);

        assert_eq!(
            config.kimi_enablement,
            crate::config::ProviderEnablement::Disabled
        );
    }

    fn minimax_account(id: &str) -> ManagedMinimaxAccountConfig {
        let now = Utc::now();
        ManagedMinimaxAccountConfig {
            id: id.to_string(),
            label: id.to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }
    }

    fn kimi_account(id: &str) -> ManagedKimiAccountConfig {
        let now = Utc::now();
        ManagedKimiAccountConfig {
            id: id.to_string(),
            label: id.to_string(),
            api_key_source: "stored".to_string(),
            created_at: now,
            updated_at: now,
            last_authenticated_at: None,
        }
    }
}
