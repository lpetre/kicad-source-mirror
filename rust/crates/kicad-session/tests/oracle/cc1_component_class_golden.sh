#!/bin/sh
# CC-1: the effective component class (FOOTPRINT::GetComponentClassAsString) of every footprint
# of the corpus boards whose project has component class assignment rules, as the reference
# pcb-dump reports it on the QA path (KI_TEST::LoadBoard) and the CLI path (BOARD_LOADER::Load).
# Writes tests/golden/cc1_component_class.txt: "<board>|<qa|loader>|<reference>|<class>".
# Replayed by tests/component_classes.rs.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
DUMP=${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}
OUT=$HERE/../golden/cc1_component_class.txt
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

: > "$OUT.tmp"

for b in component_classes component_classes_drc; do
    cp "$KICAD/qa/data/pcbnew/$b".* "$TMP/"

    for mode in qa loader; do
        flag=""
        [ "$mode" = loader ] && flag="--loader"
        # shellcheck disable=SC2086
        "$DUMP" board $flag --sections footprint "$TMP/$b.kicad_pcb" 2>/dev/null \
            | python3 -I -c '
import sys
board, mode = sys.argv[1], sys.argv[2]
ref, cls = {}, {}
for line in sys.stdin:
    line = line.rstrip("\n")
    parts = line.split("|", 2)
    if len(parts) < 3 or parts[0] != "footprint":
        continue
    field, _, value = parts[2].partition("=")
    if field == "reference":
        ref[parts[1]] = value.strip("\"")
    elif field == "component_class":
        cls[parts[1]] = value.strip("\"")
for key in ref:
    print("%s|%s|%s|%s" % (board, mode, ref[key], cls[key]))
' "$b" "$mode" >> "$OUT.tmp"
    done
done

mv "$OUT.tmp" "$OUT"
echo "wrote $OUT"
