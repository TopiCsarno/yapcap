# Command Code Provider

- `mod.rs` fetches account identity, credit balances, rolling limits, and optional subscription metadata.
- `usage.rs` maps service-reported balances, caps, and resets to shared usage types.
- `account.rs` manages private key files and explicit, read-only CLI imports.
- `login.rs` owns login state, credential validation, and persistence preparation.
- `browser.rs` starts a loopback listener and bounds its lifetime.
- `callback.rs` validates nonce-authenticated form redirects and Studio-origin JSON callbacks.
- `callback/request.rs` bounds HTTP parsing and rejects ambiguous headers; `callback/response.rs` serves redirects and static completion pages.
- `error.rs` classifies provider failures without exposing response bodies or keys.
- `tests.rs`, `tests/`, `browser/tests.rs`, `browser/tests/`, `callback/tests.rs`, `callback/tests/`, and `login/tests.rs` cover extra-credit balances, partial quotas, subscriptions, HTTP contracts, browser redirects, cancellation, storage, and credential handling.

The app login adapter is `src/app/login/flows/command_code.rs`; usage/account registration is `src/providers/adapters/command_code_adapter.rs`.
