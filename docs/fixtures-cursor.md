# Cursor fixtures

## Auth source
- Type: Browser session cookie — `WorkosCursorSessionToken` from `cursor.com`
- Cookie format: `user_XXXX::JWT` (where `::` is the separator)
- On macOS: from Safari/Chrome/Firefox cookie store
- On Linux (this machine): from Brave browser — encrypted with AES-128-CBC, key from GNOME keyring ("Brave Safe Storage"), cookie DB at `~/.config/BraveSoftware/Brave-Browser/Default/Cookies`
- Also: `~/.config/cursor/auth.json` has `accessToken` (JWT) — this works for `api2.cursor.sh` endpoints but NOT for `cursor.com` web APIs

## Request shape (web API — requires cookie)
- `GET https://cursor.com/api/usage-summary`
- `GET https://cursor.com/api/auth/me`
- Header: `Cookie: WorkosCursorSessionToken=<value>`

## Request shape (api2 — works with Bearer)
- `GET https://api2.cursor.sh/auth/full_stripe_profile`
- Header: `Authorization: Bearer <auth.json accessToken>`
- Returns billing/plan info but NOT usage percentages

## Files
- `usage_summary.json` — real response from `/api/usage-summary`. Captured 2026-04-11 (end of billing cycle — `apiPercentUsed: 100`, `remaining: 0`). Good edge case: plan exhausted.
- `auth_me.json` — real response from `/api/auth/me` (identity)
- `stripe_profile.json` — real response from `api2.cursor.sh/auth/full_stripe_profile` (plan/billing)
- `error_401.json` — response from `/api/usage-summary` with bad cookie

## Notes
- `used` and `limit` in `plan` are request counts (not tokens or dollars)
- `breakdown.included` = base plan requests, `breakdown.bonus` = bonus requests from referrals etc.
- `autoPercentUsed` vs `apiPercentUsed` vs `totalPercentUsed` are separate dimensions
- `onDemand` is for pay-as-you-go (disabled on this account, `enabled: false`)
- `teamUsage` is empty `{}` for individual accounts
- The `/api/usage?user=ID` legacy endpoint also exists but requires the same session cookie
- `billingCycleEnd` is when the plan resets
