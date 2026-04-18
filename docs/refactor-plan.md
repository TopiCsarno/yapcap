---
summary: "Minimal refactor to reach the quiet v0.1 COSMIC release — delete dead paths, fix stale/fresh, ship"
read_when:
  - "planning pre-release cleanup"
  - "deciding whether to split a file"
  - "touching provider fallback logic"
---

# YapCap Refactor Plan

Goal: reach a tagged v0.1 with an honest, finished-looking applet. Trim dead surface area, fix the one real behavior bug (stale data shown as live), then stop refactoring and ship.

Guiding rule: only refactor where it reduces release risk or removes misleading code. File splits for their own sake are post-v0.1 work.

## Final Provider Strategy

- Codex: OAuth default, Codex RPC fallback.
- Claude: OAuth default, Claude CLI fallback.
- Cursor: browser cookie path only.
- Remove Claude web path entirely.
- Remove Codex PTY/TUI parsing.
- No new providers, desktop environments, vaults, doctor command, or update checks.

## Phase 1: Delete Claude Web

- Delete `src/providers/claude_web.rs`.
- Remove `mod claude_web;` from `src/providers/mod.rs`.
- Remove `ClaudeSource::Web` from `src/providers/claude.rs`.
- Remove all `claude_web::fetch(...)` calls (see `fetch_forced_source` in `claude.rs`).
- Delete web-only variants from `ClaudeError`: `InvalidCookieHeader`, `WebOrganizationsRequest`, `WebOrganizationsEndpoint`, `DecodeWebOrganizations`, `WebUsageRequest`, `WebUsageEndpoint`, `DecodeWebUsage`, `WebAccountRequest`, `DecodeWebAccount`, `WebOrganizationMissing`, `WebUnauthorized`, `DecodeWebSchema`.
- Delete fixtures `fixtures/claude/web_account.json`, `web_organizations.json`, `web_usage.json` if no test still references them.
- Delete `src/bin/debug_claude_cookie.rs`.

## Phase 2: Delete Codex PTY

In `src/providers/codex.rs`:

- Delete `fetch_pty`, `fetch_pty_blocking`, `run_pty_command`, `parse_pty_snapshot`, and the ANSI/label helpers only used by them (`strip_ansi`, `legacy_percent_left_for_label`, `used_percent_for_compact_label`, `reset_desc_for_label`, `credits_balance`, `last_number`).
- Delete constants `PTY_STARTUP`, `PTY_TIMEOUT`, `PTY_STATUS_SETTLE`.
- Delete `CodexSource::Pty` and its match arms.
- Collapse `fetch_cli_fallback` since RPC becomes the only CLI path — call `fetch_rpc` directly from `fetch`.
- Delete PTY tests and `fixtures/codex/usage_cli.txt` if unreferenced.

## Phase 3: Drop Force-Source Env Vars and Claude Browser Config

Force-source vars were discovery aids, not release features.

- Remove `YAPCAP_CODEX_FORCE_SOURCE` and the `forced_source`/`fetch_forced_source` scaffolding in `codex.rs`.
- Remove `YAPCAP_CLAUDE_FORCE_SOURCE` and the equivalent scaffolding in `claude.rs`.
- Tests that relied on forced sources should call `normalize_oauth` / `normalize_rpc` / `claude_cli::parse_usage_snapshot` directly — they already do in most places.

`claude_browser` is dead after Phase 1 (`claude.rs:89` already discards the arg). Collapse the Claude API:

- In `src/providers/claude.rs`: delete `fetch_with_browser`, keep a single `pub async fn fetch(client: &reqwest::Client) -> Result<UsageSnapshot>` that does OAuth → CLI.
- In `src/runtime.rs`: call `claude::fetch(&client)` instead of `fetch_with_browser(&client, config.claude_browser)`.
- In `src/config.rs`: remove `claude_browser` field, remove parsing/env, remove tests for it.
- Rename `CursorBrowser` → `Browser` (or `CookieBrowser`) since it only describes Cursor's cookie source now.

This should also bring `config.rs` well under 300 lines.

## Phase 4: Fix Stale/Fresh Honesty

The badge lies in two distinct ways. Both need fixing.

**Lie 1: failed refresh shows "Live".** Evidence: `popup_view.rs:311`:

```rust
fn provider_status_badge(provider: &ProviderRuntimeState) -> &'static str {
    if !provider.enabled { "Disabled" }
    else if provider.snapshot.is_some() { "Live" }  // ← wrong after failed refresh
    else { "Error" }
}
```

After a failed refresh we keep the old snapshot (see `refresh_provider` in `runtime.rs`), which is correct. The badge just ignores `health`.

**Lie 2: cached data from hours ago shows "Live".** On app start we load from `snapshots.json` (see `load_initial_state` in `runtime.rs`). Until the first refresh completes, the popup shows yesterday's data labeled "Live · Updated 21 hours ago." That's contradictory — "Live" and a 21-hour-old timestamp can't both be right.

Fix both with a single age threshold. A snapshot older than `STALE_AFTER` (suggest 10 minutes, matching or slightly above the refresh interval) is stale regardless of why it's old.

```rust
const STALE_AFTER: chrono::Duration = chrono::Duration::minutes(10);

fn provider_status_badge(p: &ProviderRuntimeState, now: DateTime<Utc>) -> &'static str {
    if !p.enabled { return "Disabled"; }
    if p.is_refreshing { return "Refreshing"; }
    match (&p.health, &p.snapshot, p.last_success_at) {
        (ProviderHealth::Ok,    Some(_), Some(t)) if now - t < STALE_AFTER => "Live",
        (_,                     Some(_), _)                                => "Stale",
        (ProviderHealth::Error, None,    _)                                => "Error",
        (ProviderHealth::Ok,    None,    _)                                => "…",
    }
}
```

This handles both lies: a failed refresh leaves `health = Error` → "Stale"; a successful old snapshot has `now - last_success_at >= STALE_AFTER` → "Stale"; only a recent successful refresh is "Live".

Optional cleanup: add `ProviderHealth::Stale` on the Err branch in `refresh_provider` when `previous.snapshot.is_some()`. Not required since the badge computes staleness from `last_success_at` anyway. Do not introduce a separate `Fresh/Stale/Error/Disabled/Refreshing` state machine.

Also audit `model.rs::ProviderRuntimeState::status_line` for the same two lies — it currently returns `"{headline} via {source}"` with no staleness hint when a snapshot exists.

## Phase 5: Delete `debug_firefox`

`src/bin/debug_firefox.rs` is a development aid that predates the Cursor-only browser split. Delete it for a quiet release. If it turns out to be genuinely useful later, bring it back renamed as `debug_cursor_cookie`.

## Phase 6: Dependency Audit

After deletions:

```bash
cargo check
cargo test
cargo machete    # or inspect Cargo.toml manually
cargo tree --depth 1
```

Likely removals: `glob` (if unused), any fixture-only deps. Keep: `reqwest`, `tokio`, `serde`, `serde_json`, `chrono`, `dirs`, `toml`, `tracing*`, `thiserror`, `rusqlite`, `secret-service`, `aes`, `aes-gcm`, `cbc`, `base64`, `pbkdf2`, `sha1`, `tempfile`, `libcosmic`.

## Phase 7: Verify

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --bin yapcap-cosmic
./install.sh   # on a clean Pop!_OS/COSMIC env
```

Then the release-critical work in `docs/current-plan.md` (README rewrite, screenshots/GIF, tag v0.1.0) — which is what actually ships the project.

## Explicitly Not Doing Before v0.1

These were in earlier versions of this plan. They don't reduce release risk and they cost diff churn.

- **Splitting `providers/codex.rs` into a module.** After Phase 2 it drops from 802 to roughly 400 lines with clear `// === OAuth ===` / `// === CLI-RPC ===` section markers. One file is fine.
- **Splitting `browser.rs`.** 526 lines, single consumer (Cursor), works. Don't touch unless changing cookie behavior.
- **Splitting `src/error.rs`.** Shrinks meaningfully after Phase 1 and 2. `thiserror` enums read fine in one file.
- **Splitting `cosmic_app.rs` / `popup_view.rs` into a `ui/` module.** 416 and 334 lines respectively. Not big enough to justify breaking blame.
- **Multi-crate workspace, runtime command/event architecture, abstract config backends, secret vault.** Future work pulled by real need, not speculation.

If any of these files grow while you're fixing something else, split at that point — don't pre-split.

## Recommended Execution Order

1. Delete Claude web (Phase 1).
2. Delete Codex PTY (Phase 2).
3. Collapse force-source vars and Claude browser config (Phase 3).
4. Fix stale/fresh badge + status line (Phase 4).
5. Delete debug binaries (Phase 5).
6. Audit deps (Phase 6).
7. Verify (Phase 7).
8. Move on to README/screenshots/tag in `current-plan.md`.

## Main Tradeoff

Removing Codex PTY and Claude web reduces theoretical fallback coverage but improves truthfulness and maintainability. OAuth plus one proven fallback per provider is enough for a portfolio release. Dead fallback paths that maybe-work-someday make the app look unfinished; the release should look deliberate.
