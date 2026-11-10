#!/bin/sh
# Regenerates tests/golden/demo_gerber_cpp.gbr: runs the plotDemoGerber() script on the real
# GERBER_PLOTTER through the kicad-plot Gerber oracle driver
# (crates/kicad-plot/tests/oracle/build_gerber_plotter_oracle.sh builds it).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
RUST=$(cd "$HERE/../../../.." && pwd)
ORACLE=${GERBER_ORACLE:-$RUST/target/plot-oracle/gerber_plotter_oracle}
"$ORACLE" "$HERE/demo_gerber_script.txt" full | sed -n '/^F 0$/,/^F-END$/p' | sed '1d;$d' \
    > "$HERE/../golden/demo_gerber_cpp.gbr"
echo "wrote $HERE/../golden/demo_gerber_cpp.gbr"
