# Refactor Cursor Behind a Common Provider Adapter Layer

## Summary
Refactor Cursor first, but do it by introducing a shared provider adapter surface that `app.rs`, `app_refresh.rs`, `runtime.rs`, and `popup_view.rs` consume uniformly. Cursor should become the first provider fully shaped around that interface, while Codex and Claude get compatibility adapters so higher-level orchestration stops knowing Cursor-specific details.

The refactor should preserve current behavior and UI, except where small runtime-derived status cleanups are needed to remove Cursor-specific message assembly from the view layer. All Cursor implementation files stay under `src/providers/cursor/`.

## Key Changes

### 1. Introduce a shared provider interface and registry
Add a small common layer under `src/providers/` with two modules:
- `interface.rs`: shared provider-facing types used by app/runtime/UI
- `registry.rs`: the single place that matches on `ProviderId` and delegates to provider adapters

Define the shared surface as data-driven adapters, not trait objects:
- `ProviderAdapter`: provider id plus function pointers / adapter methods for config sync, account discovery, single-account refresh, account deletion, login/re-auth support, and optional background account-status refresh
- `ProviderDiscoveredAccount`: runtime-facing account descriptor with provider, account id, label, source label, auth capability flags, and an opaque provider-specific handle enum
- `ProviderAccountHandle`: enum with `Codex`, `Claude`, `Cursor` variants carrying the provider-specific managed account config needed for refresh/reconcile
- `ProviderSyncResult`: changed flag plus any startup side effects that higher layers must schedule
- `ProviderAccountActionSupport`: capability flags such as `can_delete`, `can_reauthenticate`, `supports_background_status_refresh`

Keep all provider-specific branching inside `registry.rs`. `app.rs`, `app_refresh.rs`, and `runtime.rs` should use only the shared interface plus `ProviderId`.

### 2. Make runtime/orchestration generic
Replace Cursor-specific orchestration entrypoints with provider-generic ones:
- `refresh_cursor_account_statuses_task` becomes `refresh_provider_account_statuses_task(config, state, provider)`
- `runtime::refresh_cursor_account_statuses` becomes provider-generic account-status refresh driven by adapter capability
- `runtime::reconcile_provider` and startup config sync should delegate through the provider registry instead of directly calling `codex::*`, `claude::*`, or `cursor::*`
- startup hooks such as Cursor browser discovery and debug expired-cookie simulation should be exposed as Cursor adapter startup tasks, not hardcoded in `app.rs`

Acceptance target:
- `app.rs`, `app_refresh.rs`, and `runtime.rs` contain no Cursor-specific refresh/discovery/sync logic
- the only provider-specific branching outside `src/providers/` is purely presentational text/icon selection that is intentionally UI-level

### 3. Split Cursor into small provider-local modules
Reorganize `src/providers/cursor/` so each file has one responsibility and `mod.rs` exposes only the adapter surface plus intentionally public provider APIs.

Target split:
- `mod.rs`: adapter wiring and narrow re-exports only
- `identity.rs`: normalized email, managed id, deterministic directory naming
- `storage.rs`: account metadata read/write, directory validation, commit/replace/remove helpers
- `discovery.rs`: browser discovery and import into managed account roots
- `maintenance.rs`: startup sync/cleanup, legacy pruning, pending-dir cleanup
- `refresh.rs`: cookie/session loading and network fetch
- `login.rs`: manual add / re-auth flow only
- `debug.rs`: debug-only expired-cookie simulation
- `types.rs` if needed for on-disk Cursor account file schema and provider-local structs

Move every Cursor helper currently used from higher layers behind the Cursor adapter. `app.rs` should not call Cursor account/storage helpers directly.

### 4. Normalize provider capabilities without fully rewriting Codex/Claude
Do not fully redesign Codex or Claude storage in this refactor. Instead:
- add Codex and Claude adapter shims that implement the same shared interface on top of their existing modules
- keep their existing managed-account config formats for now
- expose the same runtime-facing capabilities as Cursor where possible
- mark unsupported capabilities explicitly through `ProviderAccountActionSupport`

This keeps the scope Cursor-first while making later convergence mechanical.

### 5. Move status derivation out of the popup view
Stop assembling Cursor-specific status strings in `popup_view.rs` from raw runtime error strings.

Instead, add a shared derived status model produced before rendering:
- `ProviderStatusMessage` or equivalent derived view model containing summary text and optional action hint
- derived from provider runtime state plus account runtime states in runtime/app, not in the view
- Cursor-specific re-auth guidance comes from provider adapter status derivation, not direct checks in the view

Acceptance target:
- `popup_view.rs` renders already-derived status text and account capabilities
- it does not inspect Cursor account internals or rebuild Cursor-specific status logic

## Important Interface Changes
Add these shared interfaces under `src/providers/`:
- `ProviderAdapter`
- `ProviderDiscoveredAccount`
- `ProviderAccountHandle`
- `ProviderAccountActionSupport`
- `ProviderStatusViewModel` or equivalent shared derived status type
- generic `refresh_provider_account_statuses_task(...)`

Cursor `mod.rs` should expose only:
- the Cursor adapter registration
- Cursor login types/events if still needed by app message plumbing during this refactor
- no direct export of low-level storage/discovery helpers to higher layers

Keep all Cursor implementation files under `src/providers/cursor/`. Shared provider abstractions may live in `src/providers/interface.rs` and `src/providers/registry.rs`, but no Cursor logic should move outside the Cursor folder.

## Test Plan
- Registry tests:
  - each provider id resolves to an adapter
  - generic startup sync / reconcile / refresh paths dispatch through adapters correctly
- Cursor module tests:
  - existing normalized-email, dedupe, storage validation, discovery/import, and re-auth tests continue to pass after file split
  - debug expired-cookie simulation remains debug-only and works through the adapter path
- Runtime tests:
  - generic provider account-status refresh works for Cursor and is a no-op for providers that do not opt in
  - provider refresh and reconcile no longer require Cursor-specific branches
- UI/view-model tests:
  - Cursor re-auth status text comes from the derived provider status model, not raw `Login required`
  - account action capabilities drive the presence of delete/re-auth controls
- Regression checks:
  - startup still performs Cursor discovery, cleanup, and status refresh
  - selecting, reauthing, and refreshing Cursor accounts preserves current behavior
  - `just check` and `cargo test` pass after the refactor

## Assumptions and Defaults
- Scope is Cursor-first: fully refactor Cursor and add common adapters that Codex and Claude can use immediately without redesigning their storage models.
- The common provider interface is adapter-based and data-driven, not a trait-object architecture.
- Current user-visible behavior should remain the same unless a small shared-status derivation change is required to remove Cursor-specific UI logic.
- Cursor remains the only provider with browser discovery and background account-status refresh in this pass, but those capabilities are modeled generically so other providers can opt in later.
- No config-schema migration is required unless the implementation discovers a cleanup-only need while moving orchestration behind adapters.
