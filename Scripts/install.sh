#!/bin/zsh
set -euo pipefail

ROOT="${0:A:h:h}"
ASSUME_YES=false
WITH_LYRICS=true

for arg in "$@"; do
  case "$arg" in
    --yes|-y) ASSUME_YES=true ;;
    --without-lyrics) WITH_LYRICS=false ;;
    --help|-h)
      cat <<'EOF'
Usage: ./Scripts/install.sh [--yes] [--without-lyrics]

  --yes, -y          Install optional OmniLyrics without prompting.
  --without-lyrics   Install only Mac Info Panel.
EOF
      exit 0
      ;;
    *) print -u2 "Unknown option: $arg"; exit 64 ;;
  esac
done

if ! command -v swift >/dev/null 2>&1; then
  print -u2 "Swift toolchain is required. Install Apple Command Line Tools first: xcode-select --install"
  exit 1
fi

if "$WITH_LYRICS"; then
  OMNI_APP=""
  for candidate in "/Applications/OmniLyrics.app" "$HOME/Applications/OmniLyrics.app"; do
    if [[ -d "$candidate" ]]; then OMNI_APP="$candidate"; break; fi
  done

  if [[ -z "$OMNI_APP" ]]; then
    if ! command -v brew >/dev/null 2>&1; then
      print -u2 "OmniLyrics is optional but Homebrew is unavailable. Install Homebrew or rerun with --without-lyrics."
      exit 1
    fi
    if ! "$ASSUME_YES"; then
      printf 'Install optional OmniLyrics from zzxzzk115/tap? [y/N] '
      read -r answer
      [[ "$answer" == [yY] ]] || WITH_LYRICS=false
    fi
    if "$WITH_LYRICS"; then
      brew install --cask zzxzzk115/tap/omnilyrics
      OMNI_APP="/Applications/OmniLyrics.app"
    fi
  fi

  if "$WITH_LYRICS" && [[ -n "$OMNI_APP" ]]; then
    /usr/bin/codesign --verify --deep --strict "$OMNI_APP"
    /usr/sbin/spctl -a --type execute "$OMNI_APP"
    TEAM_ID="$(/usr/bin/codesign -d --verbose=4 "$OMNI_APP" 2>&1 | /usr/bin/sed -n 's/^TeamIdentifier=//p')"
    [[ "$TEAM_ID" == "663634Z9H8" ]] || { print -u2 "Unexpected OmniLyrics signer: $TEAM_ID"; exit 1; }
  fi
fi

CONFIG_DIR="$HOME/.config/mac-info-panel"
mkdir -p "$CONFIG_DIR"
if [[ ! -f "$CONFIG_DIR/config.json" ]]; then
  /usr/bin/ditto "$ROOT/config.example.json" "$CONFIG_DIR/config.json"
  echo "Created configuration: $CONFIG_DIR/config.json"
fi

"$ROOT/Scripts/install-launch-agent.sh"

echo "Mac Info Panel installation complete."
