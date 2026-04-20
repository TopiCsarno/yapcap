# Browser Cookie Fixtures

These fixtures are synthetic SQLite setup scripts for browser cookie stores.
They use fake values and are safe to keep in the repository.

- `chromium_cookies.sql` mirrors the current Chromium `cookies` table shape for
  the columns YapCap will need: `host_key`, `name`, `value`, and
  `encrypted_value`.
- `firefox_cookies.sql` mirrors the Firefox `moz_cookies` table shape for the
  columns YapCap will need: `host`, `name`, and `value`.

Both fixtures include one valid fake Cursor session cookie:

```text
WorkosCursorSessionToken = cursor-test-session-token
```

They also include decoy rows so parser tests can prove that host and cookie name
filtering are both working.
