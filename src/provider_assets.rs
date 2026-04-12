use crate::model::ProviderId;
use cosmic::widget::icon::{self, Handle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderIconVariant {
    Default,
    Reversed,
}

pub fn provider_icon_handle(provider: ProviderId, variant: ProviderIconVariant) -> Handle {
    let bytes = match (provider, variant) {
        (ProviderId::Codex, ProviderIconVariant::Default) => {
            include_bytes!("../resources/providers/codex.svg").as_slice()
        }
        (ProviderId::Codex, ProviderIconVariant::Reversed) => {
            include_bytes!("../resources/providers/codex-reversed.svg").as_slice()
        }
        (ProviderId::Claude, ProviderIconVariant::Default) => {
            include_bytes!("../resources/providers/claude.svg").as_slice()
        }
        (ProviderId::Claude, ProviderIconVariant::Reversed) => {
            include_bytes!("../resources/providers/claude-reversed.svg").as_slice()
        }
        (ProviderId::Cursor, ProviderIconVariant::Default) => {
            include_bytes!("../resources/providers/cursor.svg").as_slice()
        }
        (ProviderId::Cursor, ProviderIconVariant::Reversed) => {
            include_bytes!("../resources/providers/cursor-reversed.svg").as_slice()
        }
    };

    icon::from_svg_bytes(bytes)
}
