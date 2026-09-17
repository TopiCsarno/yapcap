# Login Flows

- `mod.rs` declares and re-exports provider flow implementations; the parent `src/app/login/mod.rs` defines the shared flow contract and dispatch.
- `antigravity.rs`, `claude.rs`, `codex.rs`, `copilot.rs`, `gemini.rs`, and `grok.rs` adapt browser/device OAuth flows, including supported imports and reauthentication.
- `kimi.rs`, `minimax.rs`, `opencode_go.rs`, `synthetic.rs`, and `zai.rs` implement API-key account flows.

Provider-specific network and account-storage details belong in `src/providers/`; these modules adapt them to UI login state.
