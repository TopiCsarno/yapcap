# Provider Adapters

Each adapter implements the shared provider contract for one provider:

- `antigravity_adapter.rs`, `claude_adapter.rs`, `codex_adapter.rs`, `copilot_adapter.rs`, `cursor_adapter.rs`, and `grok_adapter.rs` cover OAuth or local-token providers.
- `gemini_adapter.rs` covers Google Code Assist usage.
- `kimi_adapter.rs`, `minimax_adapter.rs`, `opencode_go_adapter.rs`, `synthetic_adapter.rs`, and `zai_adapter.rs` cover API-key providers.

Adapters coordinate provider modules; endpoint parsing and credential schemas belong in the provider directory.
