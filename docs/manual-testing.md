# Manual Testing Guide

## Desktop Environment Testing

### What varies by DE

The provider logic, auth, cache, and CLI probing are all desktop-agnostic — they behave identically regardless of which DE is running. The surface that actually varies per DE is narrow:

- **Secret Service backend** — GNOME Keyring on GNOME, KWallet on KDE
- **Panel/tray integration** — whether the applet icon appears and behaves correctly
- **Applet lifecycle** — how the DE handles the COSMIC applet process

You do not need to re-run provider QA scenarios for every DE. Run those once on COSMIC. For other DEs, focus on the keyring and panel surface.

---

### Option 1: One VM, multiple DE sessions (recommended starting point)

Install a single Debian or Fedora VM and add multiple desktop environments on top of each other:

```bash
# Debian/Ubuntu — add KDE on top of GNOME
sudo apt install kde-plasma-desktop

# Fedora — add KDE on top of GNOME
sudo dnf install @kde-desktop
```

Switch between sessions at the login screen (GDM or SDDM). Same OS, same packages, same installed yapcap binary — just a different session. Fast to set up, low disk overhead.

---

### Option 2: Nested compositor (fastest iteration)

Run a nested Wayland compositor as a window inside your current session. No VM, near-zero overhead. Good enough for testing Secret Service integration, D-Bus behaviour, and basic applet rendering.

**GNOME (Mutter nested):**
```bash
dbus-run-session -- mutter --wayland --nested
```

**KDE (KWin windowed):**
```bash
dbus-run-session -- kwin_wayland --windowed
```

This gives you a GNOME or KDE Wayland environment as a window inside your existing desktop. The nested session has its own D-Bus, its own keyring daemon, and its own panel — sufficient for testing whether yapcap's Secret Service path works against GNOME Keyring vs KWallet without rebooting or switching sessions.

---

### Option 3: Distrobox for packaging verification

Not useful for GUI testing, but good for verifying the deb/rpm install, file paths, and `yapcap doctor` output on different distro bases without affecting your host:

```bash
distrobox create --name fedora-test --image fedora:latest
distrobox enter fedora-test
# install the rpm, run doctor, verify paths
```

---

### What to test per DE

| Check | GNOME | KDE | Notes |
|---|---|---|---|
| Secret Service stores and retrieves a secret | GNOME Keyring | KWallet | Core auth path |
| Keyring locked state surfaces correctly in UI | ✓ | ✓ | Lock the keyring manually and observe applet state |
| Keyring unlock repair action works | ✓ | ✓ | Trigger unlock from applet repair action |
| Vault fallback works when keyring killed | ✓ | ✓ | `pkill gnome-keyring-daemon` or equivalent |
| Panel icon appears | ✓ | ✓ | |
| Popup opens and closes | ✓ | ✓ | |
| `yapcap doctor` reports correct keyring backend | ✓ | ✓ | |

---

### Chromium cookie decryption

Chromium stores its cookie encryption key under different D-Bus paths depending on the DE:

- GNOME: `Chrome Safe Storage` via GNOME Keyring Secret Service
- KDE: `Chrome Safe Storage` via KWallet

Both paths must be tested if the Cursor or Claude web cookie import path is being verified. The nested compositor approach is sufficient for this — spin up a nested session with the target DE, log into the browser inside it, and run the cookie import flow.

---

## Provider Manual QA

These scenarios come from the spec (section 7.3) and should be run on COSMIC. They do not need to be repeated per DE.

### Codex

- Fresh system with no Codex install
- Codex installed but not logged in
- Codex logged in with valid auth file
- Auth file has expired token and valid refresh token
- Auth file has expired token and invalid refresh token
- RPC unavailable but PTY available
- PTY output includes update prompt
- Weekly window missing in response
- Credits-only response in OAuth mode
- Network offline with cached snapshot present

### Claude

- Claude CLI not installed
- Credentials file missing
- Credentials file present without `user:profile`
- OAuth success path
- OAuth unauthorized, delegated refresh succeeds
- OAuth unauthorized, delegated refresh fails, CLI fallback succeeds
- CLI `/usage` parse with weekly and model lanes
- CLI `/usage` parse with session only
- Web fallback success with imported sessionKey
- All sources unavailable

### Cursor

- Browser logged in, import succeeds
- Cached cookie header valid
- Cached cookie header expired, browser fallback succeeds
- Browser cookie names missing, domain-cookie fallback succeeds
- Browser cookies stale, stored session valid
- Stored session stale and cleared on auth failure
- Manual cookie header valid
- Manual cookie header invalid
- `usage-summary` missing optional fields
- `auth/me` endpoint temporarily failing

---

## Secret Backend Manual QA

These scenarios should be run on each supported keyring backend.

- Secret Service available and unlocked — normal startup path
- Secret Service locked at startup — applet shows `Keyring locked`, repair action unlocks it
- Secret Service unavailable — applet offers vault setup or runs in memory-only mode
- Vault configured, passphrase correct — secrets load on unlock
- Vault configured, passphrase wrong — error surfaced, no crash
- Vault reset — all secrets destroyed, applet returns to memory-only
- Secret Service becomes available mid-session after starting in memory-only — migration prompt appears, no automatic migration
