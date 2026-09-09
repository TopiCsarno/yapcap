# Providers

- `mod.rs` declares provider modules.
- `interface.rs` defines the shared adapter contract and provider-facing types.
- `registry.rs` exposes provider capabilities, account actions, and usage fetches to the runtime/UI.
- `adapters.rs` and `adapters/` map each provider implementation to the shared interface.
- `google_oauth.rs` contains shared Google OAuth helpers.
- `opencode_auth.rs` reads compatible OpenCode credentials for explicit imports/prefills.
- `antigravity/`, `claude/`, `codex/`, `copilot/`, `cursor/`, `gemini/`, `grok/`, `kimi/`, `minimax/`, and `opencode_go/` implement provider-specific behavior.
