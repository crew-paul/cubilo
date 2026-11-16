#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile=${1:-release}
case "$profile" in
  release) cargo build -p studio-app --release --locked ;;
  debug) cargo build -p studio-app --locked ;;
  *) echo 'usage: scripts/package-macos.sh [debug|release]' >&2; exit 2 ;;
esac
bundle="target/$profile/Cubilo.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/studio-app" "$bundle/Contents/MacOS/.Cubilo.new"
mv "$bundle/Contents/MacOS/.Cubilo.new" "$bundle/Contents/MacOS/Cubilo"
cp assets/fonts/OFL-NanumGothic.txt "$bundle/Contents/Resources/"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>app.cubilo.studio</string>
<key>CFBundleName</key><string>Cubilo</string>
<key>CFBundleDisplayName</key><string>큐빌로</string>
<key>CFBundleExecutable</key><string>Cubilo</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>15.2</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf '%s\n' "$bundle"
