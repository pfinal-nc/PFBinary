#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EXT="${PF_EXT:-$ROOT/php/modules/pfserialize.so}"
PHP=(php -d "extension=$EXT")

echo "==> smoke"
"${PHP[@]}" "$ROOT/php/tests/smoke.php"
echo
echo "==> regression"
"${PHP[@]}" "$ROOT/php/tests/regression.php"
echo
echo "ALL PHP TESTS PASS"
