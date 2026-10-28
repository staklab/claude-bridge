#!/bin/bash
# Build dependencies are required only on the build host, never on a user's Mac.
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TASK_DIST="${BRIDGE_DIST:-$TASK_ROOT/dist}"
TASK_VERSION="0.2.0"
TASK_TEAM="${BRIDGE_TEAM_ID:-}"
TASK_IDENTITY="${BRIDGE_SIGN_IDENTITY:-}"
TASK_APP="$TASK_DIST/Claude Bridge.app"
TASK_STAGE="$(mktemp -d -t claude-bridge-build)"
trap 'rm -rf "$TASK_STAGE"' EXIT
cd "$TASK_ROOT"
export MACOSX_DEPLOYMENT_TARGET=13.0
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$TASK_ROOT=/build/claude-bridge --remap-path-prefix=$HOME=/build/user"
mkdir -p "$TASK_DIST"
if [ -n "$TASK_IDENTITY" ]; then
  case "$TASK_IDENTITY" in "Developer ID Application:"*) ;; *) echo "A Developer ID Application identity is required for distribution." >&2; exit 1;; esac
  test -n "$TASK_TEAM"
fi
if [ "${BRIDGE_SKIP_CARGO:-0}" != "1" ]; then
  cargo build --release --locked --target aarch64-apple-darwin --bins
fi
TASK_BIN="$TASK_ROOT/target/aarch64-apple-darwin/release"
if [ "${BRIDGE_NATIVE_BINARIES:-0}" = "1" ]; then TASK_BIN="$TASK_ROOT/target/release"; fi
TASK_BUNDLE="$TASK_STAGE/Claude Bridge.app"
mkdir -p "$TASK_BUNDLE/Contents/MacOS" "$TASK_BUNDLE/Contents/Resources/bin"
/usr/bin/swiftc -swift-version 5 -parse-as-library -target arm64-apple-macos13.0 -O \
  -framework SwiftUI -framework AppKit -framework CryptoKit \
  macos/ClaudeBridge/ClaudeBridge.swift macos/ClaudeBridge/Updater.swift macos/ClaudeBridge/TerminalLauncher.swift \
  -o "$TASK_BUNDLE/Contents/MacOS/Claude Bridge"
cp "$TASK_BIN/claude-gpt" "$TASK_BIN/claude-code-proxy" "$TASK_BUNDLE/Contents/Resources/bin/"
for TASK_BINARY in "$TASK_BUNDLE/Contents/Resources/bin/claude-gpt" "$TASK_BUNDLE/Contents/Resources/bin/claude-code-proxy"; do
  /usr/bin/lipo -verify_arch arm64 "$TASK_BINARY"
done
cp LICENSE "$TASK_BUNDLE/Contents/Resources/LICENSE"
python3 scripts/macos/notices.py "$TASK_BUNDLE/Contents/Resources/Third Party Notices.txt"
cp docs/CLAUDE_BRIDGE_USER_GUIDE.md "$TASK_BUNDLE/Contents/Resources/User Guide.md"
cp macos/ClaudeBridge/Assets/*.svg "$TASK_BUNDLE/Contents/Resources/"
/usr/bin/plutil -create xml1 "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundleIdentifier -string org.claudebridge.desktop "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundleName -string 'Claude Bridge' "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundleDisplayName -string 'Claude Bridge' "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundleExecutable -string 'Claude Bridge' "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundlePackageType -string APPL "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundleShortVersionString -string "$TASK_VERSION" "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundleVersion -string "$TASK_VERSION" "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert LSMinimumSystemVersion -string 13.0 "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert LSApplicationCategoryType -string public.app-category.developer-tools "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert NSHumanReadableCopyright -string 'Independent open-source software. Not an OpenAI or Anthropic product.' "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert CFBundleIconFile -string AppIcon "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/plutil -insert BridgePublicDistribution -bool NO "$TASK_BUNDLE/Contents/Info.plist"
/usr/bin/swift scripts/macos/icon.swift "$TASK_STAGE/AppIcon.iconset"
/usr/bin/iconutil -c icns "$TASK_STAGE/AppIcon.iconset" -o "$TASK_BUNDLE/Contents/Resources/AppIcon.icns"
TASK_EFFECTIVE_IDENTITY="${TASK_IDENTITY:--}"
for TASK_BINARY in "$TASK_BUNDLE/Contents/Resources/bin/claude-gpt" "$TASK_BUNDLE/Contents/Resources/bin/claude-code-proxy"; do
  /usr/bin/codesign --force --options runtime --sign "$TASK_EFFECTIVE_IDENTITY" ${TASK_IDENTITY:+--timestamp} "$TASK_BINARY"
done
/usr/bin/codesign --force --options runtime --sign "$TASK_EFFECTIVE_IDENTITY" ${TASK_IDENTITY:+--timestamp} "$TASK_BUNDLE/Contents/MacOS/Claude Bridge"
# Hash the signed binary bytes; install verifies these before copying them.
/usr/bin/swift scripts/macos/manifest.swift "$TASK_BUNDLE" "$TASK_VERSION" "$TASK_TEAM" "${BRIDGE_UPDATE_FEED:-}"
if [ -n "$TASK_IDENTITY" ]; then
  # Only a successfully notarized build may claim public distribution readiness.
  test -n "${BRIDGE_NOTARY_PROFILE:-}" || { echo "BRIDGE_NOTARY_PROFILE is required for distribution." >&2; exit 1; }
  /usr/bin/plutil -replace BridgePublicDistribution -bool YES "$TASK_BUNDLE/Contents/Info.plist"
fi
/usr/bin/codesign --force --options runtime --sign "$TASK_EFFECTIVE_IDENTITY" ${TASK_IDENTITY:+--timestamp} "$TASK_BUNDLE"
/usr/bin/codesign --verify --deep --strict "$TASK_BUNDLE"
if [ -n "$TASK_IDENTITY" ]; then
  TASK_SIGNED_TEAM="$(/usr/bin/codesign -d --verbose=4 "$TASK_BUNDLE" 2>&1 | /usr/bin/awk -F= '/^TeamIdentifier=/{print $2}')"
  test "$TASK_SIGNED_TEAM" = "$TASK_TEAM" || { echo 'Signing team does not match BRIDGE_TEAM_ID.' >&2; exit 1; }
  /usr/bin/ditto -c -k --keepParent "$TASK_BUNDLE" "$TASK_STAGE/ClaudeBridge.zip"
  /usr/bin/xcrun notarytool submit "$TASK_STAGE/ClaudeBridge.zip" --keychain-profile "$BRIDGE_NOTARY_PROFILE" --wait
  /usr/bin/xcrun stapler staple "$TASK_BUNDLE"
  /usr/sbin/spctl --assess --type execute "$TASK_BUNDLE"
fi
mkdir "$TASK_STAGE/disk"
cp -R "$TASK_BUNDLE" "$TASK_STAGE/disk/Claude Bridge.app"
ln -s /Applications "$TASK_STAGE/disk/Applications"
cp docs/CLAUDE_BRIDGE_USER_GUIDE.md "$TASK_STAGE/disk/使用说明.md"
if [ -z "$TASK_IDENTITY" ]; then
  printf '%s\n' 'LOCAL TEST BUILD ONLY — Developer ID signing and Apple notarization are pending. Do not disable Gatekeeper. This build is not ready for public download distribution.' > "$TASK_STAGE/disk/本地测试构建.txt"
fi
TASK_DMG="$TASK_DIST/Claude Bridge.dmg"
/usr/bin/hdiutil create -volname 'Claude Bridge' -srcfolder "$TASK_STAGE/disk" -ov -format UDZO "$TASK_STAGE/Claude Bridge.dmg"
if [ -n "$TASK_IDENTITY" ]; then
  /usr/bin/codesign --sign "$TASK_IDENTITY" --timestamp "$TASK_STAGE/Claude Bridge.dmg"
  /usr/bin/xcrun notarytool submit "$TASK_STAGE/Claude Bridge.dmg" --keychain-profile "$BRIDGE_NOTARY_PROFILE" --wait
  /usr/bin/xcrun stapler staple "$TASK_STAGE/Claude Bridge.dmg"
  /usr/bin/xcrun stapler validate "$TASK_STAGE/Claude Bridge.dmg"
fi
if [ -d "$TASK_APP" ]; then rm -rf "$TASK_APP"; fi
cp -R "$TASK_BUNDLE" "$TASK_APP"
cp "$TASK_STAGE/Claude Bridge.dmg" "$TASK_DMG"
mkdir -p "$TASK_DIST/native"
cp "$TASK_BUNDLE/Contents/Resources/bin/claude-gpt" "$TASK_BUNDLE/Contents/Resources/bin/claude-code-proxy" "$TASK_DIST/native/"
(cd "$TASK_DIST" && /usr/bin/shasum -a 256 'Claude Bridge.dmg' native/claude-gpt native/claude-code-proxy > SHA256SUMS)
echo "Built: $TASK_APP"
echo "Built: $TASK_DMG"
if [ -z "$TASK_IDENTITY" ]; then echo 'LOCAL TEST BUILD: public distribution is not ready (Developer ID and notarization missing).'; fi
