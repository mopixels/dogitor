#!/usr/bin/env bash
# Builds a downloadable, universal (Apple Silicon + Intel) Dogitor .dmg:
#
#   dist/Dogitor-<version>-universal.dmg
#
# containing Dogitor.app, an /Applications shortcut to drag it onto, and a
# short "READ ME FIRST" with the one-time Gatekeeper approval + dependencies.
# Runs locally on any Mac (Intel or Apple Silicon) and in CI
# (.github/workflows/release.yml). Needs only Rust + Xcode command line tools;
# the missing target is added via rustup if needed.
#
# The app is ad-hoc signed, not notarized (no paid Apple Developer account),
# so a downloaded copy needs a one-time "Open Anyway" — see the README.
set -euo pipefail

cd "$(dirname "$0")/.."

# Cross-compiling needs rustup's toolchain. A Homebrew `rust` formula in
# /usr/local/bin or /opt/homebrew/bin would otherwise shadow it and fail with
# "can't find crate for `std`" for the non-native target.
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
command -v rustup >/dev/null || { echo "rustup is required (https://rustup.rs)"; exit 1; }

APP_NAME="Dogitor"
BIN_NAME="dogitor_gui"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

WORK="target/release-mac"
STAGE="$WORK/dmg"
DIST="dist"
DMG="$DIST/$APP_NAME-$VERSION-universal.dmg"

for t in "${TARGETS[@]}"; do
    if ! rustup target list --installed | grep -qx "$t"; then
        echo "==> Adding Rust target $t"
        rustup target add "$t"
    fi
    echo "==> Building $BIN_NAME for $t"
    # Match LSMinimumSystemVersion in assemble-app.sh.
    MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked --bin "$BIN_NAME" --target "$t"
done

echo "==> Creating universal binary"
mkdir -p "$WORK"
lipo -create \
    "target/aarch64-apple-darwin/release/$BIN_NAME" \
    "target/x86_64-apple-darwin/release/$BIN_NAME" \
    -output "$WORK/$BIN_NAME"
lipo -info "$WORK/$BIN_NAME"

rm -rf "$STAGE"
mkdir -p "$STAGE" "$DIST"
scripts/assemble-app.sh "$WORK/$BIN_NAME" "$STAGE/$APP_NAME.app"
ln -s /Applications "$STAGE/Applications"

cat > "$STAGE/READ ME FIRST.txt" <<'TXT'
Dogitor — install
=================

1. Drag Dogitor onto the Applications folder.

2. Install the two helper tools Dogitor needs (Homebrew: https://brew.sh):

       brew install ffmpeg mediamtx

3. Open Dogitor from Applications. The first time, macOS will say it
   "cannot verify" the app, because it isn't signed with a paid Apple
   developer certificate. Click Done (not "Move to Bin"), then:

       System Settings → Privacy & Security → scroll down →
       "Dogitor was blocked…" → Open Anyway → enter your password

   You only need to do this once.

   (Terminal alternative: xattr -dr com.apple.quarantine /Applications/Dogitor.app)

4. Dogitor opens into Settings: paste your Telegram bot token, generate an
   API token, pick your camera, then Save & Start. Allow Camera and
   Microphone access when asked.

Full docs: https://github.com/mopixels/dogitor
TXT

echo "==> Building $DMG"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
(cd "$DIST" && shasum -a 256 "$(basename "$DMG")") | tee "$DMG.sha256"

echo
echo "==> Done: $DMG"
