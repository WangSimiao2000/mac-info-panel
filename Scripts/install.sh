#!/bin/zsh
set -euo pipefail

ROOT="${0:A:h:h}"
WITH_LYRICS=true

for arg in "$@"; do
  case "$arg" in
    --yes|-y) ;;
    --without-lyrics) WITH_LYRICS=false ;;
    --help|-h)
      cat <<'EOF'
Usage: ./Scripts/install.sh [--yes] [--without-lyrics]

  --yes, -y          Kept for non-interactive compatibility.
  --without-lyrics   Install without the media-control dependency.
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

if "$WITH_LYRICS" && ! command -v media-control >/dev/null 2>&1; then
  if ! command -v brew >/dev/null 2>&1; then
    print -u2 "Lyrics require media-control. Install Homebrew or rerun with --without-lyrics."
    exit 1
  fi
  brew install media-control
fi

if "$WITH_LYRICS"; then
  media-control test
fi

CONFIG_DIR="$HOME/.config/mac-info-panel"
mkdir -p "$CONFIG_DIR"
if [[ ! -f "$CONFIG_DIR/config.json" ]]; then
  /usr/bin/ditto "$ROOT/config.example.json" "$CONFIG_DIR/config.json"
  echo "Created configuration: $CONFIG_DIR/config.json"
fi

"$ROOT/Scripts/install-launch-agent.sh"

echo "Mac Info Panel installation complete."
