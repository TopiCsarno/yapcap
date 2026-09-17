# Synthetic Provider

- `mod.rs` fetches and normalizes Synthetic subscription quotas from the `/v2/quotas` endpoint.
- `account.rs` manages labeled API-key accounts.
- `login.rs` implements API-key login and reauthentication.
- `opencode.rs` supports optional one-time OpenCode key prefill.
- `storage.rs` reads and writes private API-key files; account metadata lives in COSMIC configuration.
