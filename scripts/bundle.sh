#!/usr/bin/env bash
# Build LucAPI and wrap it in a macOS .app bundle.
#   ./scripts/bundle.sh            -> dist/LucAPI.app
#   ./scripts/bundle.sh --install  -> also copies it to /Applications
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

APP="dist/LucAPI.app"
ICON_SRC="assets/logo.png"

cargo build --release
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/lucapi "$APP/Contents/MacOS/LucAPI"
chmod +x "$APP/Contents/MacOS/LucAPI"

# ---- icon -----------------------------------------------------------------
cp assets/LucAPI.icns "$APP/Contents/Resources/LucAPI.icns"

# ---- metadata -------------------------------------------------------------
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>es</string>
    <key>CFBundleExecutable</key><string>LucAPI</string>
    <key>CFBundleIconFile</key><string>LucAPI</string>
    <key>CFBundleIdentifier</key><string>dev.lucapi.desktop</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>LucAPI</string>
    <key>CFBundleDisplayName</key><string>LucAPI</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.10.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
printf 'APPL????' > "$APP/Contents/PkgInfo"

# An ad-hoc signature keeps Gatekeeper quiet for a locally built app.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

if [[ "${1:-}" == "--install" ]]; then
    rm -rf "/Applications/LucAPI.app"
    cp -R "$APP" /Applications/
    echo "Installed /Applications/LucAPI.app"
fi
echo "Built $APP"
