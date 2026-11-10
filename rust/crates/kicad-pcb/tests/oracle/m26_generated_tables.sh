#!/bin/sh
# Regenerate tests/golden/m26_generated_tables.txt with the reference kicad-cli (package M26).
# Works on copies in a scratch directory (kicad-cli rewrites the board in place).
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
CLI=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
WORK=${M26_WORK:-$(mktemp -d)}
python3 -I "$HERE/m26_generated_tables.py" "$CLI" "$WORK" "$HERE/../golden/m26_generated_tables.txt"
echo "outputs in $WORK"
