# Providers

- `mod.rs` declares provider modules.
- `interface.rs` defines the shared adapter contract and provider-facing types.
- `registry.rs` exposes provider capabilities, account actions, and usage fetches to the runtime/UI.
- `registry/` contains provider-contract and account-facts tests.
- `adapters.rs` and `adapters/` map each provider implementation to the shared interface.
- `google_oauth.rs` contains shared Google OAuth helpers.
- `opencode_auth.rs` reads compatible OpenCode credentials for explicit imports/prefills.
- `antigravity/`, `claude/`, `codex/`, `command_code/`, `copilot/`, `cursor/`, `gemini/`, `grok/`, `kimi/`, `minimax/`, `opencode_go/`, and `zai/` implement provider-specific behavior.
