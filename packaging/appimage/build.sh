#!/bin/sh
# Build a portable AppImage from an existing release binary.
# Usage: packaging/appimage/build.sh [version]
# Requires: appimagetool in PATH, release binary at target/release/whatsapp-tui
set -eu
cd -- "$(dirname -- "$0")/../.."

VERSION="${1:-0.2.0}"
APPDIR="dist/AppDir"
rm -rf "$APPDIR" dist/*.AppImage dist/*.AppImage.sha256
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps"

if [ ! -x target/release/whatsapp-tui ]; then
  echo "missing target/release/whatsapp-tui, run ./scripts/check.sh release first" >&2
  exit 1
fi

cp target/release/whatsapp-tui "$APPDIR/usr/bin/"
cp packaging/whatsapp-tui.desktop "$APPDIR/whatsapp-tui.desktop"
cp packaging/whatsapp-tui.desktop "$APPDIR/usr/share/applications/"
cp packaging/icon.svg "$APPDIR/whatsapp-tui.svg"
cp packaging/icon.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/whatsapp-tui.svg"
cp packaging/appimage/AppRun "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"

# Minimal AppStream metadata helps appimagetool lint pass.
cat > "$APPDIR/whatsapp-tui.appdata.xml" <<XML
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>whatsapp-tui</id>
  <metadata_license>MIT</metadata_license>
  <name>Whatsatui</name>
  <summary>Personal WhatsApp terminal client</summary>
  <description><p>Pane-based WhatsApp TUI in Rust and Ratatui.</p></description>
  <launchable type="desktop-id">whatsapp-tui.desktop</launchable>
  <provides><binary>whatsapp-tui</binary></provides>
</component>
XML

ARCH="$(uname -m)"
APPIMAGE="dist/whatsatui-${VERSION}-${ARCH}.AppImage"
appimagetool "$APPDIR" "$APPIMAGE"
sha256sum "$APPIMAGE" | tee "$APPIMAGE.sha256"
