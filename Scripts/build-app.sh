#!/bin/zsh
set -euo pipefail

ROOT="${0:A:h:h}"
cd "$ROOT"

/usr/bin/swift build -c release
BIN_DIR="$(/usr/bin/swift build -c release --show-bin-path)"
APP="$ROOT/.build/Mac Info Panel.app"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
/usr/bin/ditto "$BIN_DIR/MacInfoPanel" "$APP/Contents/MacOS/MacInfoPanel"
/usr/bin/ditto "$ROOT/Resources/Info.plist" "$APP/Contents/Info.plist"
/usr/bin/codesign --force --deep --sign - "$APP"

echo "$APP"
