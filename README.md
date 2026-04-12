# YapCap

YapCap is a COSMIC panel applet for showing usage state for Codex, Claude Code, and Cursor.

Current implementation:
- Codex via OAuth token from `~/.codex/auth.json`
- Claude via OAuth token from `~/.claude/.credentials.json`
- Cursor via Brave browser cookie import from `~/.config/BraveSoftware/Brave-Browser/Default/Cookies`

Build:

```bash
cargo build --release
```

Run locally:

```bash
./target/release/yapcap-cosmic
```

Install for local COSMIC testing:

1. Copy `target/release/yapcap-cosmic` somewhere on your `PATH`, for example `~/.local/bin/`.
2. Copy `resources/com.topi.YapCap.desktop` to `~/.local/share/applications/`.
3. Make sure the desktop entry `Exec=` line points to the installed binary path if needed.
4. Restart the panel session or log out and back in so COSMIC rescans applets.

Notes:
- Logs are written under the XDG state directory, typically `~/.local/state/yapcap/logs/yapcap.log`.
- Config is stored at `~/.config/yapcap/config.toml`.
- Snapshot cache is stored at `~/.cache/yapcap/snapshots.json`.
