#!/bin/bash
# Regenerates tests/golden/gerbers_layer_args.txt with the reference kicad-cli (docs/ORACLE.md):
# `pcb export gerbers -l <case>` on a copy of qa/data/pcbnew/via_stacks.kicad_pcb (6 copper
# layers); records the exit code, the "Invalid layer name" messages and the plotted layers
# ("P <file name suffix> <extension>", in plot order).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
O=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
W=$(mktemp -d)
export KICAD_ORACLE_HOME=$W/home
cp "$KICAD/qa/data/pcbnew/via_stacks.kicad_pcb" "$W/"
cd "$W"
cases=( "" "F.Cu" "F.Cu,B.Cu" " F.Cu , B.Cu " "F.Cu,,B.Cu" "F.Cu," ",F.Cu" "*.Cu" "*" "*In.Cu" "F&B.Cu"
        "*.Mask,*.SilkS" "*.Silkscreen,*.Courtyard,*.Adhesive" "F.Silkscreen,B.Adhesive" "Bogus,F.Cu"
        "f.cu" "Edge.Cuts,Margin,User.1,User.Drawings,Dwgs.User" "B.Fab,F.Fab,*.Fab"
        "*.Paste,*.Adhes,*.CrtYd" "In1.Cu,In4.Cu,In5.Cu" "User.9,Rescue,B.Courtyard" "F.Cu;B.Cu" )
out=$HERE/../golden/gerbers_layer_args.txt
: > "$out"
for c in "${cases[@]}"; do
    rm -rf g
    "$O" pcb export gerbers -o g -l "$c" via_stacks.kicad_pcb > o.txt 2>/dev/null
    echo "CASE [$c] rc=$?" >> "$out"
    sed -e "s#'g/via_stacks-\(.*\)\.\(g[a-z0-9]*\)'\.#\1 \2#" -e 's/^Plotted to /P /' o.txt >> "$out"
done
rm -rf "$W"
echo "wrote $out"
