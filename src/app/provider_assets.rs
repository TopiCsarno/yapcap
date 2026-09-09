// SPDX-License-Identifier: MPL-2.0

use crate::model::ProviderId;
use cosmic::widget::icon::{self, Handle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderIconVariant {
    Default,
    Reversed,
}

pub fn provider_icon_handle(provider: ProviderId, variant: ProviderIconVariant) -> Handle {
    let bytes: &[u8] = match (provider, variant) {
        (ProviderId::Codex, ProviderIconVariant::Default) => {
            include_bytes!("../../resources/providers/codex.svg")
        }
        (ProviderId::Codex, ProviderIconVariant::Reversed) => {
            include_bytes!("../../resources/providers/codex-reversed.svg")
        }
        (ProviderId::Claude, _) => {
            include_bytes!("../../resources/providers/claude-color.svg")
        }
        (ProviderId::Cursor, ProviderIconVariant::Default) => {
            include_bytes!("../../resources/providers/cursor.svg")
        }
        (ProviderId::Cursor, ProviderIconVariant::Reversed) => {
            include_bytes!("../../resources/providers/cursor-reversed.svg")
        }
        (ProviderId::Gemini, _) => {
            include_bytes!("../../resources/providers/gemini-color.svg")
        }
        (ProviderId::Copilot, ProviderIconVariant::Default) => {
            include_bytes!("../../resources/providers/copilot.svg")
        }
        (ProviderId::Copilot, ProviderIconVariant::Reversed) => {
            include_bytes!("../../resources/providers/copilot-reversed.svg")
        }
        (ProviderId::Minimax, ProviderIconVariant::Default) => {
            include_bytes!("../../resources/providers/minimax.svg")
        }
        (ProviderId::Minimax, ProviderIconVariant::Reversed) => {
            include_bytes!("../../resources/providers/minimax-reversed.svg")
        }
        (ProviderId::Kimi, ProviderIconVariant::Default) => {
            include_bytes!("../../resources/providers/kimi.svg")
        }
        (ProviderId::Kimi, ProviderIconVariant::Reversed) => {
            include_bytes!("../../resources/providers/kimi-reversed.svg")
        }
        (ProviderId::Antigravity, _) => {
            include_bytes!("../../resources/providers/antigravity-color.svg")
        }
        (ProviderId::OpenCodeGo, ProviderIconVariant::Default) => {
            include_bytes!("../../resources/providers/opencode-go.svg")
        }
        (ProviderId::OpenCodeGo, ProviderIconVariant::Reversed) => {
            include_bytes!("../../resources/providers/opencode-go-reversed.svg")
        }
        (ProviderId::Grok, ProviderIconVariant::Default) => {
            include_bytes!("../../resources/providers/grok.svg")
        }
        (ProviderId::Grok, ProviderIconVariant::Reversed) => {
            include_bytes!("../../resources/providers/grok-reversed.svg")
        }
    };

    icon::from_svg_bytes(bytes)
}

pub fn app_icon_handle() -> Handle {
    icon::from_svg_bytes(include_bytes!("../../resources/icon.svg"))
}

pub fn provider_icon_variant() -> ProviderIconVariant {
    if cosmic::theme::is_dark() {
        ProviderIconVariant::Reversed
    } else {
        ProviderIconVariant::Default
    }
}
