// SPDX-License-Identifier: MPL-2.0

use crate::config::{
    ManagedClaudeAccountConfig, ManagedCodexAccountConfig, ManagedCursorAccountConfig,
};
use crate::model::ProviderId;

#[derive(Debug, Clone)]
pub struct ProviderAccountActionSupport {
    pub can_delete: bool,
    pub can_reauthenticate: bool,
    pub supports_background_status_refresh: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderDiscoveredAccount {
    pub provider: ProviderId,
    pub account_id: String,
    pub label: String,
    pub action_support: ProviderAccountActionSupport,
    pub handle: ProviderAccountHandle,
}

#[derive(Debug, Clone)]
pub enum ProviderAccountHandle {
    Codex(ManagedCodexAccountConfig),
    Claude(ManagedClaudeAccountConfig),
    Cursor(ManagedCursorAccountConfig),
}
