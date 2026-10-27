#!/bin/zsh
set -euo pipefail

ROOT="${0:A:h:h}"
"$ROOT/Scripts/build-app.sh"

SOURCE_APP="$ROOT/.build/Mac Info Panel.app"
INSTALL_DIR="$HOME/Applications"
INSTALL_APP="$INSTALL_DIR/Mac Info Panel.app"
LABEL="io.github.WangSimiao2000.MacInfoPanel"
LEGACY_LABEL="cn.mickeymiao.mac-info-panel"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"

mkdir -p "$INSTALL_DIR" "$HOME/Library/LaunchAgents"
rm -rf "$INSTALL_APP"
/usr/bin/ditto "$SOURCE_APP" "$INSTALL_APP"

/usr/bin/plutil -create xml1 "$PLIST"
/usr/bin/plutil -replace Label -string "$LABEL" "$PLIST"
/usr/bin/plutil -replace ProgramArguments -json "[\"$INSTALL_APP/Contents/MacOS/MacInfoPanel\"]" "$PLIST"
/usr/bin/plutil -replace RunAtLoad -bool true "$PLIST"
/usr/bin/plutil -replace ProcessType -string Interactive "$PLIST"
/usr/bin/plutil -replace StandardOutPath -string "$HOME/Library/Logs/MacInfoPanel.log" "$PLIST"
/usr/bin/plutil -replace StandardErrorPath -string "$HOME/Library/Logs/MacInfoPanel.error.log" "$PLIST"

/bin/launchctl bootout "gui/$(id -u)/$LEGACY_LABEL" 2>/dev/null || true
/bin/rm -f "$HOME/Library/LaunchAgents/$LEGACY_LABEL.plist"
/bin/launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
for _ in {1..20}; do
  /usr/bin/pgrep -x MacInfoPanel >/dev/null || break
  /bin/sleep 0.25
done
/bin/launchctl bootstrap "gui/$(id -u)" "$PLIST"
/bin/launchctl kickstart "gui/$(id -u)/$LABEL" 2>/dev/null || true

echo "Installed: $INSTALL_APP"
echo "LaunchAgent: $PLIST"
