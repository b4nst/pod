#!/usr/bin/env bash
# Wrap the podlink binary in an ad-hoc signed "pod link.app" and zip it.
#   scripts/bundle-macos.sh <podlink-binary> <out.zip> [version]
set -euo pipefail
bin=$1; out=$2; version=${3:-0.0.0}
work=$(mktemp -d)
app="$work/pod link.app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/podlink"
cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>podlink</string>
  <key>CFBundleIdentifier</key><string>ch.banst.podlink</string>
  <key>CFBundleName</key><string>pod link</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version#v}</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
codesign --force --deep -s - "$app"
rm -f "$out"
ditto -c -k --keepParent "$app" "$out"
rm -rf "$work"
echo "$out"
