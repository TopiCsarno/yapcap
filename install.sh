#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="yapcap-cosmic"
APP_ID="com.topi.YapCap"

INSTALL_BIN_DIR="${INSTALL_BIN_DIR:-$HOME/.local/bin}"
INSTALL_APPS_DIR="${INSTALL_APPS_DIR:-$HOME/.local/share/applications}"
INSTALL_ICONS_DIR="${INSTALL_ICONS_DIR:-$HOME/.local/share/icons/hicolor/scalable/apps}"
DESKTOP_SOURCE="$ROOT_DIR/resources/$APP_ID.desktop"
DESKTOP_TARGET="$INSTALL_APPS_DIR/$APP_ID.desktop"
ICON_SOURCE="$ROOT_DIR/resources/icon.svg"
ICON_TARGET="$INSTALL_ICONS_DIR/$APP_ID.svg"
BUILT_BIN_SOURCE="$ROOT_DIR/target/release/$BIN_NAME"
BUNDLED_BIN_SOURCE="$ROOT_DIR/$BIN_NAME"
BIN_TARGET="$INSTALL_BIN_DIR/$BIN_NAME"

mkdir -p "$INSTALL_BIN_DIR" "$INSTALL_APPS_DIR" "$INSTALL_ICONS_DIR"

if [[ -x "$BUNDLED_BIN_SOURCE" ]]; then
  BIN_SOURCE="$BUNDLED_BIN_SOURCE"
else
  echo "Building release binary..."
  cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"
  BIN_SOURCE="$BUILT_BIN_SOURCE"
fi

echo "Installing binary to $BIN_TARGET"
install -m 0755 "$BIN_SOURCE" "$BIN_TARGET"

echo "Installing desktop entry to $DESKTOP_TARGET"
sed "s|^Exec=.*|Exec=$BIN_TARGET|" "$DESKTOP_SOURCE" > "$DESKTOP_TARGET"
chmod 0644 "$DESKTOP_TARGET"

echo "Installing icon to $ICON_TARGET"
install -m 0644 "$ICON_SOURCE" "$ICON_TARGET"

cat <<EOF
Install complete.

Binary:
  $BIN_TARGET

Desktop entry:
  $DESKTOP_TARGET

Icon:
  $ICON_TARGET

If COSMIC does not pick it up immediately, restart the panel session or log out and back in.
EOF
