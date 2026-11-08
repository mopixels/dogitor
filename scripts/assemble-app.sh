#!/usr/bin/env bash
# Hand-assembles a Dogitor.app around an already-built `dogitor_gui` binary and
# ad-hoc signs it. Shared by bundle-mac.sh (local install) and release-mac.sh
# (downloadable .dmg) so both produce the identical bundle.
#
#   scripts/assemble-app.sh <path/to/dogitor_gui binary> <path/to/Dogitor.app>
#
# A hand-rolled Info.plist is required (rather than a generic generator)
# because it must carry NSCameraUsageDescription/NSMicrophoneUsageDescription:
# without those keys macOS denies camera/mic access outright instead of
# prompting.
set -euo pipefail

BIN="$1"
APP_DIR="$2"
APP_NAME="Dogitor"
BUNDLE_ID="com.dogitor.app"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

echo "==> Assembling $APP_DIR (v$VERSION)"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "$BIN" "$APP_DIR/Contents/MacOS/$APP_NAME"
cp "$ROOT/assets/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"

cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSUIElement</key>
    <false/>
    <key>NSCameraUsageDescription</key>
    <string>Dogitor needs camera access to record video clips when a loud sound is detected.</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>Dogitor listens to the microphone to detect loud sounds like barking.</string>
</dict>
</plist>
PLIST

# Ad-hoc signature: no Apple ID needed. Mandatory for arm64 binaries to run at
# all, but NOT enough to pass Gatekeeper on a downloaded copy — users approve
# it once via System Settings → Privacy & Security → Open Anyway.
echo "==> Ad-hoc code signing"
codesign --force --deep -s - "$APP_DIR"
