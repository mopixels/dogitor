#!/usr/bin/env bash
# Builds `dogitor_gui` in release mode for this Mac, assembles Dogitor.app
# (see assemble-app.sh) and installs it into /Applications. For a
# downloadable universal .dmg, use release-mac.sh instead.
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="Dogitor"
OUT_DIR="target/release/bundle"
APP_DIR="$OUT_DIR/$APP_NAME.app"
BIN_NAME="dogitor_gui"

echo "==> Building $BIN_NAME (release)"
cargo build --release --bin "$BIN_NAME"

scripts/assemble-app.sh "target/release/$BIN_NAME" "$APP_DIR"

echo "==> Installing to /Applications"
INSTALLED_APP="/Applications/$APP_NAME.app"
rm -rf "$INSTALLED_APP"
cp -R "$APP_DIR" "$INSTALLED_APP"

echo
echo "==> Done: $INSTALLED_APP"
echo
echo "Dogitor is now in your Applications folder with its own icon — open it"
echo "from Launchpad, Spotlight (Cmd+Space, type Dogitor), or Finder > Applications,"
echo "same as any other Mac app. No Terminal needed from here on. Drag it onto"
echo "the Dock from Applications if you want it to stay there."
echo
echo "A locally built app isn't quarantined, so Gatekeeper shouldn't object. If it"
echo "does: System Settings > Privacy & Security > Open Anyway."
echo
echo "Note: ad-hoc signing means rebuilding from source can force a fresh"
echo "camera/mic permission prompt (the signature identity isn't stable across"
echo "rebuilds). If that gets annoying, sign with a free Apple ID 'Personal"
echo "Team' identity instead — grants then persist across rebuilds."
