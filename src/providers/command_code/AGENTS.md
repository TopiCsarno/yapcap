# Command Code Provider

- `mod.rs` fetches account identity, credit balances, rolling limits, and optional subscription metadata.
- `usage.rs` maps service-reported balances, caps, and resets to shared usage types.
- `account.rs` manages private key files and explicit, read-only CLI imports.
- `login.rs` owns login state, credential validation, and persistence preparation.
- `browser.rs` starts a loopback listener and bounds its lifetime.
- `callback.rs` validates Studio-origin JSON callbacks, state nonces, and request limits.
- `error.rs` classifies provider failures without exposing response bodies or keys.
- `tests.rs`, `tests/balances.rs`, `tests/http.rs`, `browser/tests.rs`, `callback/tests.rs`, and `login/tests.rs` cover extra-credit balances, HTTP contracts, browser callbacks, cancellation, storage, and credential handling.

The app login adapter is `src/app/login/flows/command_code.rs`; usage/account registration is `src/providers/adapters/command_code_adapter.rs`.
