# YapCap QA Plan

Manual test plan for v0.4.0. Run against both Native (`just install`) and Flatpak (`just flatpak-install`) builds unless noted.

Paths used below:

**Native** (default XDG layout on typical Linux installs):

- Config: `~/.config/cosmic/com.topi.YapCap/v400/`
- Cache: `~/.cache/yapcap/snapshots.json`
- Accounts + logs: `~/.local/state/yapcap/` (e.g. `…/logs/yapcap.log`)

**Flatpak** (app id `com.topi.YapCap`; paths use passwd `pw_dir` as `~`):

- Config: same COSMIC config schema `v400` dir (manifest mounts `~/.config/cosmic`)
- Cache: `~/.var/app/com.topi.YapCap/cache/yapcap/snapshots.json`
- Accounts + logs: `~/.var/app/com.topi.YapCap/data/yapcap/`

Do not expect the Flatpak build to use `~/.local/state/yapcap/` for YapCap data—that is native-only.

---

## 1. Fresh install

- [ ] `just clear-all-data` then install. All three provider tabs visible with "Login required" state (not hidden).
- [ ] Existing `v300` COSMIC settings are not loaded after the `v400` schema boundary; users must re-add accounts.
- [ ] Existing account directories, snapshot caches, and logs are not automatically deleted by the schema boundary and may remain orphaned.
- [ ] Settings → General → About shows correct version and dist label ("Native" or "Flatpak").
- [ ] Panel icon renders without clipping or overflow.

---

## 2. Panel icon styles

In Settings → General, cycle through all four panel icon styles and verify the panel updates immediately each time:

- [ ] `Logo and bars` — provider logo + two usage bars visible.
- [ ] `Bars only` — no logo, just bars.
- [ ] `Logo and percent` — logo + one percentage number.
- [ ] `Percent only` — only percentage, no logo. Tooltip in Settings explains it shows the first usage window.

---

## 3. General settings

- [ ] Autorefresh interval buttons — set each value, restart, confirm the interval persisted.
- [ ] Reset time format `relative` — usage windows show "Resets in Xd Xh".
- [ ] Reset time format `absolute` — windows show "Resets tomorrow at …" or day + time.
- [ ] Usage amount format `used` — bars and labels show consumed quota.
- [ ] Usage amount format `left` — bars and labels flip to remaining quota.
- [ ] Settings survive an app restart (kill and re-open).

---

## 4. Theme

- [ ] Switch COSMIC to dark theme — provider icons switch to dark-panel variant.
- [ ] Switch COSMIC to light theme — provider icons switch to reversed/light variant.
- [ ] Change COSMIC accent colour — accent fill on selected tabs and rows updates without restart.

---

## 5. Update checker

- [ ] About section shows "Checking for updates…" briefly on startup.
- [ ] If up to date, shows "Up to date".
- [ ] Simulate update available: `YAPCAP_DEBUG_UPDATE_AVAILABLE=1 cargo run` — red dot on Settings gear, General tab, and About title. Hovering dots shows "Update available".
- [ ] "Check again" appears and works when update check fails.

---

## 6. Codex

### 6.1 Add account
- [ ] Settings → Codex → Add account opens browser OAuth flow.
- [ ] Cancel during login returns to normal add-account state with no partial account stored.
- [ ] Successful login stores account under native `~/.local/state/yapcap/codex-accounts/` or Flatpak `~/.var/app/com.topi.YapCap/data/yapcap/codex-accounts/`.
- [ ] Stored directory contains `metadata.json` and `tokens.json`; `metadata.json` has `email` and `provider_account_id`; `tokens.json` has `access_token`, `refresh_token`, and `expires_at`.
- [ ] Duplicate login (same email) updates the existing account directory, not a second entry.
- [ ] New account is selected immediately in single-account mode.

### 6.2 Usage display
- [ ] Session window (5h) shows used/left percent and reset time.
- [ ] Weekly window (7d) shows used/left percent and reset time.
- [ ] If credits balance present, cost card is visible.
- [ ] Pace indicator marker visible on bars with both `reset_at` and `window_seconds`.

### 6.3 Token refresh
> minor: reauthenticate badge should be on a new line 


- [ ] Corrupt `tokens.json` → `access_token` only, remove `refresh_token`. Verify "Login required" state after one failed refresh.
- [ ] Set `expires_at` to one minute in the past with a valid `refresh_token`. On next refresh, YapCap should transparently renew the token and fetch usage without showing an error. Verify `tokens.json` `expires_at` is updated.
- [ ] Set `expires_at` far in the past and set `refresh_token` to a junk value. Verify `ActionRequired` state ("Login" badge) and re-auth prompt in Settings.

### 6.4 Remove account
- [ ] Remove account from Settings — account directory deleted, provider shows empty state.

---

## 7. Claude

### 7.1 Add account
- [ ] Settings → Claude → Add account opens browser OAuth flow and prompts for authentication code paste.
- [ ] Pasting a wrong or malformed code shows an explicit plain-language error ("paste the authentication code or full callback URL"); existing accounts are untouched.
- [ ] Successful add stores account under native `~/.local/state/yapcap/claude-accounts/` or Flatpak `~/.var/app/com.topi.YapCap/data/yapcap/claude-accounts/`.
- [ ] Stored directory contains `metadata.json` and `tokens.json`; `tokens.json` has `access_token`, `refresh_token`, and `expires_at`.
- [ ] Duplicate email upserts the existing account rather than creating a second entry.
- [ ] New account is selected immediately in single-account mode.

### 7.2 Usage display
- [ ] 5h session window and 7d weekly window visible.
- [ ] Max plan accounts: Sonnet, Opus, and Cowork model-specific windows visible.
- [ ] Pro plan accounts: model-specific windows absent.
- [ ] Extra usage / credits cost card visible when present.
- [ ] `utilization=0` + `resets_at=null` on the 5h window shows "Reset" label, not an error.

### 7.3 Token refresh
- [ ] Set `expires_at` to one minute in the past with a valid `refresh_token`. Verify silent refresh on next cycle. Verify `tokens.json` `expires_at` is updated.
- [ ] Replace `refresh_token` with junk. Verify `ActionRequired` badge and re-auth icon in Settings.
- [ ] Per-account re-auth: click re-auth icon → complete OAuth with the same email → usage refreshes immediately.
- [ ] Per-account re-auth with a different email → rejected with error, existing account unchanged.

### 7.4 Rate limiting
- [ ] Observe `RateLimited` behaviour: provider shows rate-limited message; if `Retry-After` header present, "(retry in Xm)" appended.
- [ ] After the backoff window passes, the next refresh clears `rate_limit_until`.

### 7.5 Remove account
- [ ] Remove from Settings — account directory deleted, provider shows empty state.

---

## 8. Cursor

> tokens.json stores the same token as both access token as refresh token

> when having multiple accounts the logged out account shows as "reauth-needed"

> looks like when user logs out of the account the tokens don't work anymore

> when logged out I get this error message: database missing key: cursorAuth/accessToken

### 8.1 Add account (SQLite scan flow)
- [ ] Settings → Cursor → Add account triggers a scan of `~/.config/Cursor/User/globalStorage/state.vscdb`.
- [ ] If Cursor is not installed or the state DB is absent, a clear error is shown and no account is stored.
- [ ] Successful scan stores account under native `~/.local/state/yapcap/cursor-accounts/<opaque-id>/` or Flatpak `~/.var/app/com.topi.YapCap/data/yapcap/cursor-accounts/<opaque-id>/`.
- [ ] Stored `tokens.json` contains `access_token`, `token_id`, `expires_at`, and `refresh_token`.
- [ ] Stored `metadata.json` contains `email` (non-empty), display name, and plan.
- [ ] Directory name is opaque (`cursor-<millis>-<pid>` format) and does not embed the email.
- [ ] Duplicate scan for the same email replaces the existing managed account directory rather than creating a second entry.
- [ ] New account is selected immediately in single-account mode.
- [ ] Config `cursor_managed_accounts` entry has `id`, `email`, and `managed_account_root`; no bearer tokens.

### 8.2 Usage display
- [ ] Total and API windows shown on the thin panel bars; Auto + Composer windows are skipped on the panel.
- [ ] Full popup shows all usage windows.
- [ ] Billing cycle end date drives reset time.
- [ ] Membership type shown in identity/plan badge.

### 8.3 Token refresh
- [ ] Set `expires_at` in `tokens.json` to one minute in the past with a valid `refresh_token`. On next usage cycle, YapCap calls the refresh endpoint, writes rotated tokens, and fetches usage without showing an error. Verify `expires_at` updated in `tokens.json`.
- [ ] Replace `refresh_token` with a junk value and set `expires_at` in the past. Verify `LoginRequired` state ("Login required" / "Re-auth needed" badge).
- [ ] HTTP 429 or network error during refresh → transient; stale snapshot stays visible with error status.

### 8.4 Expired session simulation
- [ ] `YAPCAP_DEBUG_CURSOR_EXPIRED_COOKIE=1 just run` — existing Cursor account shows `Re-auth needed` badge in account header and in Settings.
- [ ] Provider status text instructs user to go to Settings and reauthenticate.
- [ ] Re-scanning (Add account) with valid Cursor credentials clears the simulated expired state and triggers a fresh usage fetch.

### 8.5 Remove account
- [ ] Remove from Settings — YapCap-owned account directory deleted, Cursor's own `~/.config/Cursor` files are untouched, provider shows empty state.

---

## 9. Multi-account

- [ ] Add a second account for any provider.
- [ ] `Show all accounts` toggle appears only when the provider has more than one account.
- [ ] `Show all accounts` off — single active account column in popup.
- [ ] `Show all accounts` on — one column per selected account side by side. Popup width expands by 420 px per additional column.
- [ ] Panel bars expand horizontally: one two-bar group per selected account.
- [ ] Unloaded accounts show 0% fill in panel until their snapshot arrives.
- [ ] Switching the active account in single-account mode triggers a refresh for only that provider, not a global refresh.

---

## 10. Stale / error states

- [ ] Kill network (`nmcli networking off`). Trigger a refresh. Verify "No internet connection. Showing cached data; information is not up to date." message. Cached usage data still visible. Re-enable network, verify Live badge returns.
- [ ] Wait 11 minutes without refreshing (or set refresh interval to max and advance clock). Verify account badge switches from Live to Stale. Status line appends "(stale)".
- [ ] Cold start with a cache from >10 minutes ago. Verify Stale badge on startup, not "Live · Updated 21 hours ago".
- [ ] Corrupt `~/.cache/yapcap/snapshots.json` (truncate or write invalid JSON). Verify app starts cleanly with Loading state.

---

## 11. Provider enable/disable

- [ ] Disable a provider via its settings toggle — provider tab disappears from popup nav.
- [ ] All provider-specific settings below the toggle are dimmed and non-interactive when disabled.
- [ ] Re-enable — tab reappears and a refresh is triggered.
- [ ] Fresh install with `auto_init_pending`: all providers enabled even with no accounts.

---

## 12. Popup sizing

- [ ] Single-account provider: popup is 420 px wide.
- [ ] Two-account provider: popup is 840 px wide.
- [ ] Switching from a two-account tab to a one-account tab shrinks popup immediately.
- [ ] Switching from provider view to Settings shrinks to settings width.
- [ ] Content taller than 1080 px: body scrolls, header/nav/footer stay fixed.
- [ ] Header, nav, and footer stay centred at 420 px even in wide multi-account popup.

---

## 13. Accounts removed from filesystem

- [ ] Manually delete a provider account directory from the YapCap data tree (`~/.local/state/yapcap/<provider>-accounts/` native, or `~/.var/app/com.topi.YapCap/data/yapcap/<provider>-accounts/` Flatpak). Trigger a refresh. Verify the provider surfaces "Login required" or empty state rather than showing a stale snapshot indefinitely.

---

## 14. Config state file manipulation

- [ ] Delete cached snapshots (native `~/.cache/yapcap/snapshots.json`, Flatpak `~/.var/app/com.topi.YapCap/cache/yapcap/snapshots.json`). Restart. Verify app starts with Loading state and fetches fresh data.
- [ ] Delete the COSMIC config dir (`just clear-config`). Restart. Verify defaults apply: all providers enabled, refresh interval 300s, relative reset time, used amount format.
- [ ] Leave an older `~/.config/cosmic/com.topi.YapCap/v300/` config in place. Restart the current build and verify `v400` defaults are used instead.
- [ ] Manually edit config to add a non-existent account id to `selected_codex_account_ids`. Restart. Verify graceful fallback to first valid account or Login Required — no crash.
- [ ] Set `refresh_interval_seconds = 5` in config. Verify it is clamped to 10s at runtime (not 5s).

---

## 15. Logging

- [ ] Native: verify `~/.local/state/yapcap/logs/yapcap.log`. Flatpak: verify `~/.var/app/com.topi.YapCap/data/yapcap/logs/yapcap.log`. Each is written during a normal session for that build.
- [ ] Verify no bearer tokens, access tokens, cookie values, or refresh tokens appear in the log.
- [ ] `RUST_LOG=debug just run` — debug output in terminal, still no credentials in log file.

---

## 16. Flatpak-specific

- [ ] Install via `just flatpak-install`. YapCap appears in COSMIC applet list.
- [ ] About section shows "Flatpak" dist label.
- [ ] OAuth flows (Codex, Claude) open the system browser correctly from inside the sandbox.
- [ ] Cursor add-account: Flatpak sandbox can read `~/.config/Cursor/User/globalStorage/state.vscdb` (manifest grants `--filesystem=~/.config/Cursor:ro`). Scan succeeds and account is stored.
- [ ] Account state for the Flatpak build lives under `~/.var/app/com.topi.YapCap/data/yapcap/` (not `~/.local/state/yapcap/`).
- [ ] `just flatpak-run` launches the installed Flatpak version.
- [ ] Native install (`just install`) About section shows "Native".
