#!/bin/sh
# Build only the AppKit adapter; pass a separately built/registered guest worker.
set -eu
: "${X05_BUILD_ROOT:=$HOME/.cache/olm-emu/build-X05}"
mkdir -p "$X05_BUILD_ROOT/tmp" "$X05_BUILD_ROOT/MacCustomUI.app/Contents/MacOS"
export TMPDIR="$X05_BUILD_ROOT/tmp"
cd "$(dirname "$0")/../.."
swiftc -swift-version 5 -module-cache-path "$X05_BUILD_ROOT/swift-cache" tools/mac-custom-ui/main.swift -o "$X05_BUILD_ROOT/MacCustomUI.app/Contents/MacOS/mac-custom-ui"
cat > "$X05_BUILD_ROOT/MacCustomUI.app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>mac-custom-ui</string>
<key>CFBundleIdentifier</key><string>org.aexcompat.custom-ui</string>
<key>CFBundleName</key><string>AEXCompat Custom UI</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf '%s\n' "$X05_BUILD_ROOT/MacCustomUI.app/Contents/MacOS/mac-custom-ui"
