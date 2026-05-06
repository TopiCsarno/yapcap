<div align="center">

# YapCap

**A native COSMIC panel applet that tracks AI coding quota for Codex, Claude Code, and Cursor.**

<img src="resources/screenshots/screenshot.png" alt="YapCap panel applet" width="780" />

[![CI](https://github.com/TopiCsarno/yapcap/actions/workflows/ci.yml/badge.svg)](https://github.com/TopiCsarno/yapcap/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/TopiCsarno/yapcap?label=release)](https://github.com/TopiCsarno/yapcap/releases/latest)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue.svg)](LICENSE)

[Report a bug](https://github.com/TopiCsarno/yapcap/issues)

</div>

---

## What it does

YapCap lives in your COSMIC panel and shows how much of your AI coding quota you've used — without sending anything to a third party. All data is fetched directly from provider APIs using accounts you add in YapCap. No telemetry, no cloud sync, no separate account needed.


## Highlights

- **Three providers**
    - **Codex** — 5h/weekly windows + credits
    - **Claude Code** — session/weekly/extra usage
    - **Cursor** — plan usage + billing cycle end
- **Multi-account view** — add, switch, and remove accounts per provider. Turn on **Show all accounts** to lay out each selected account side by side in the popup and show one usage-bar group per account in the panel.
- **In-app login** — guided login flows for Codex, Claude, and Cursor without leaving YapCap or opening a terminal
- **Explicit accounts** — credentials are added through YapCap and stored under YapCap-owned account directories
- **Configurable panel** — logo+bars, bars only, logo+%, or %-only; used/left toggle; relative or absolute reset times
- **COSMIC themes** — popup and panel respect your system theme, accent, and icon context
<!-- - **Available in the COSMIC Store** — YapCap can be installed through the COSMIC Store as a flatpak package which recieves auto-updates.  -->

## Screenshots

<table>
<tr>
<td align="center" valign="top" colspan="2" width="50%">

<strong>Popup — usage detail</strong><br />
<img src="resources/screenshots/screenshot-detail.png" alt="YapCap popup showing Codex usage for two accounts" />

</td>
<td align="center" valign="top" width="25%">

<strong>Settings — General</strong><br />
<img src="resources/screenshots/screenshot-settings-general.png" alt="YapCap General settings" />

</td>
<td align="center" valign="top" width="25%">

<strong>Settings — Accounts</strong><br />
<img src="resources/screenshots/screenshot-settings-codex.png" alt="YapCap Codex account settings" />

</td>
</tr>
<tr>
<td colspan="4" align="center">

<strong>COSMIC system theme</strong><br />
YapCap follows your COSMIC system theme—the popup and panel pick up light or dark mode and accent colors from your desktop appearance settings.

</td>
</tr>
<tr>
<td align="center" valign="top" width="25%">
<img src="resources/screenshots/screenshot_theme1.png" alt="YapCap popup matching COSMIC system theme (1)" />
</td>
<td align="center" valign="top" width="25%">
<img src="resources/screenshots/screenshot_theme2.png" alt="YapCap popup matching COSMIC system theme (2)" />
</td>
<td align="center" valign="top" width="25%">
<img src="resources/screenshots/screenshot_theme3.png" alt="YapCap popup matching COSMIC system theme (3)" />
</td>
<td align="center" valign="top" width="25%">
<img src="resources/screenshots/screenshot_theme4.png" alt="YapCap popup matching COSMIC system theme (4)" />
</td>
</tr>
</table>

## Install

### Flatpak

<!--
After YapCap is listed in [pop-os/cosmic-flatpak](https://github.com/pop-os/cosmic-flatpak) and the COSMIC Store, install from the **cosmic** Flatpak remote:

```bash
flatpak remote-add --if-not-exists --user cosmic https://apt.pop-os.org/cosmic/cosmic.flatpakrepo
flatpak install --user cosmic com.topi.YapCap
```

You can also open **COSMIC Store**, search for **YapCap**, and install from there (add the `cosmic` remote if the store asks for it).

-->

Manifests follow the same layout as [pop-os/cosmic-flatpak](https://github.com/pop-os/cosmic-flatpak) Rust applets: `com.system76.Cosmic.BaseApp`, primary **`git`** source, plus `packaging/cargo-sources.json` from `Cargo.lock`. The repo manifest keeps the store-style GitHub source, while `just flatpak-build` and `just flatpak-install` generate a temporary local manifest from the active local Git branch and export that same branch. Uncommitted edits are not included; commit them first.

From a clone:

```bash
just flatpak-build
```

Install (runs `flatpak-build` first, then exports and installs):

```bash
just flatpak-install
```

Re-export and install from an existing `build-dir` without running `flatpak-builder` again:

```bash
just flatpak-install-only
```

Manual build (user Flatpak install, Flathub deps):

```bash
flatpak-builder --user --install-deps-from=flathub --install --force-clean build-dir packaging/com.topi.YapCap.json
```

That manual command uses the store-style manifest source from GitHub. Use the `just` recipes for local branch testing.

Packaging details are in `docs/spec.md` § Packaging; regenerate `packaging/cargo-sources.json` after lockfile changes with [flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/blob/master/cargo/flatpak-cargo-generator.py) and `Cargo.lock`.

### apt (Debian/Ubuntu/Pop!\_OS)

```bash
sudo apt install ./yapcap_*.deb
```

### rpm (Fedora/openSUSE)

```bash
sudo rpm -i ./yapcap_*.rpm
```

Download packages from the [latest release](https://github.com/TopiCsarno/yapcap/releases/latest).

### From source

Requires COSMIC development dependencies and a Rust toolchain.

```bash
git clone https://github.com/TopiCsarno/yapcap
cd yapcap
just install
```

## Quickstart

1. After installing, restart your COSMIC session (log out and back in).
2. Add **YapCap** from the panel applet picker.
3. On first launch, add accounts explicitly from **Settings → [Provider] → Add account**. Codex and Claude use browser OAuth, and Cursor scans the local Cursor IDE account state.
4. Click the panel button to open the popup.
5. To add more accounts or switch between them, open the popup → **Settings → [Provider]**.

---

## Accounts

Each provider supports multiple accounts. Manage them from the popup under **Settings → [Provider]**.

- **Add account** — triggers the provider's own login flow: Codex browser OAuth, native Claude OAuth in the browser, or Cursor IDE account scanning, without leaving YapCap.
- **Switch account** — tap any account row to make it active; the panel and popup update immediately.
- **Remove account** — deletes only YapCap's copy of the credentials. Provider accounts and host app configs are never touched.

Each provider keeps at most one account per email address.

## Panel styles

Configured under **Settings → General**:

| Style | What's shown |
| --- | --- |
| Logo + bars | Provider icon and two compact usage bars (default) |
| Bars only | Two usage bars, no icon |
| Logo + percent | Provider icon and the headline window as a percentage |
| Percent only | Headline percentage only |

## Display options

Also under **Settings → General**:

- **Usage format** — show quota as *used* (how much you've consumed) or *left* (how much remains).
- **Reset time format** — relative durations (`Resets in 2d 4h`) or absolute local times (`Resets Wednesday at 8:25 AM`).
- **Auto-refresh interval** — how often YapCap polls the provider APIs in the background.

Usage bars include a pace indicator: a vertical marker shows expected usage for the elapsed portion of the window so you can see at a glance whether you're running ahead or behind.

## Updates

YapCap checks GitHub for a new release on startup. If one is available, a red dot appears on the Settings icon and a link to the release page appears in **Settings → About**. No automatic download or install.

## Privacy

YapCap stores provider credentials under YapCap-owned account storage and calls provider APIs directly over HTTPS. Claude OAuth refresh uses Anthropic’s token endpoint, not the Claude CLI. Codex login runs inside a temporary YapCap-owned CLI home and is converted into YapCap account storage. Logs never contain credentials, bearer tokens, or cookie values — if you find one leaking, please file a bug.

## Troubleshooting

- **Applet doesn't appear after install** — restart the COSMIC session (log out and back in).
- **Auth error on Codex** — open **Settings → Codex**, remove the account if needed, and add it again to complete the managed login flow.
- **Auth error on Claude** — open **Settings → Claude**, remove the account if needed, and add it again to complete the browser OAuth flow.
- **Cursor shows no data** — open **Settings → Cursor** and add or re-authenticate the account with the managed browser flow.
- **Stale data** — a transient failure keeps the last good snapshot visible and marks it stale. Click **Refresh now** once the network or provider is back.

Logs (native): `~/.local/state/yapcap/logs/yapcap.log`. Logs (Flatpak): `~/.var/app/com.topi.YapCap/data/yapcap/logs/yapcap.log`.

## File locations

**Native** (typical XDG defaults):

| Path | Purpose |
| --- | --- |
| `~/.config/cosmic/com.topi.YapCap/v*/` | Settings (provider toggles, accounts, display options) |
| `~/.cache/yapcap/snapshots.json` | Cached usage state (loaded on startup) |
| `~/.local/state/yapcap/`{`codex`,`claude`,`cursor`}`-accounts/` | Managed credential copies |
| `~/.local/state/yapcap/logs/yapcap.log` | Log output |

**Flatpak** (`com.topi.YapCap`): YapCap cache and state live only under `~/.var/app/com.topi.YapCap/` — use `cache/yapcap/` for snapshots and `data/yapcap/` for accounts and logs. The manifest mounts host `~/.config/cosmic` read-write for COSMIC app settings (not `xdg-config/cosmic`, for compatibility with Flatpak path resolution).

## Limitations

- COSMIC only. No GNOME, KDE, or tray fallback.
- No historical charts, notifications, or cost analytics.
- Three providers only for now.

## License

MPL-2.0 — see [LICENSE](LICENSE).
