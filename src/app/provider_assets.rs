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
    };

    icon::from_svg_bytes(bytes)
}

pub fn provider_icon_variant() -> ProviderIconVariant {
    if cosmic::theme::is_dark() {
        ProviderIconVariant::Reversed
    } else {
        ProviderIconVariant::Default
    }
}
