#!/usr/bin/env bash
# Build + install pfserialize into PHP extension_dir (may need sudo).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PHP_DIR="$ROOT/php"

bash "$PHP_DIR/build.sh"

EXT_DIR="$(php-config --extension-dir)"
SO="$PHP_DIR/modules/pfserialize.so"
INI_DIR="$(php -r 'echo PHP_CONFIG_FILE_SCAN_DIR ?: "";' 2>/dev/null || true)"

echo "==> Installing $SO → $EXT_DIR/"
if [[ -w "$EXT_DIR" ]]; then
  cp "$SO" "$EXT_DIR/pfserialize.so"
else
  sudo cp "$SO" "$EXT_DIR/pfserialize.so"
fi

if [[ -n "$INI_DIR" && -d "$INI_DIR" ]]; then
  DEST="$INI_DIR/20-pfserialize.ini"
  echo "==> Writing $DEST"
  if [[ -w "$INI_DIR" ]]; then
    cp "$PHP_DIR/pfserialize.ini" "$DEST"
  else
    sudo cp "$PHP_DIR/pfserialize.ini" "$DEST"
  fi
  echo "Installed. Verify: php -m | grep pfserialize"
else
  echo "No PHP_CONFIG_FILE_SCAN_DIR; enable manually:"
  echo "  extension=$EXT_DIR/pfserialize.so"
  echo "  (see $PHP_DIR/pfserialize.ini)"
fi
