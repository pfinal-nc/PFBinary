#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PHP_DIR="$ROOT/php"

echo "==> phpize / configure / make (pure-C, no Rust link)"
cd "$PHP_DIR"
if [[ -f Makefile ]]; then
  make distclean >/dev/null 2>&1 || true
fi
phpize
./configure --enable-pfserialize
make -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

echo "==> Built: $PHP_DIR/modules/pfserialize.so"
echo "Tests:  bash $PHP_DIR/tests/run_all.sh"
echo "Install (optional): bash $PHP_DIR/install.sh"
echo "Run smoke:"
echo "  php -d extension=$PHP_DIR/modules/pfserialize.so $PHP_DIR/tests/smoke.php"
