#!/bin/sh
# Package LIVE-2, verification d, second half: the reference kicad-cli and the port's kicad-rs
# run `pcb drc --format report --severity-all --units mm` on every board that
# `LIVE2_EXPORT=<dir> cargo test -p kicad-session --test live2_drc export_edited` saved (edited
# in random commits, finalize(), full refill), and the reports are compared without the date
# and file name lines. Prints one line per board and a summary.
#
#   tests/oracle/live2_cli_compare.sh <dir> [kicad-rs binary]
#
# Each board directory is copied to a scratch directory first (kicad-cli writes side files).
# One reference process at a time (PORTING.md section 7).
set -u
DIR=$1
RS=${2:-$(dirname "$0")/../../../../target/release/kicad-rs}
CLI=${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}
SCRATCH=$(mktemp -d)
export KICAD_ORACLE_HOME="$SCRATCH/home"
same=0; differ=0

norm() {
    grep -v -e '^\*\* Created on' -e '^\*\* Drc report for' "$1"
}

for b in "$DIR"/*/; do
    name=$(basename "$b")
    rm -rf "$SCRATCH/ref" "$SCRATCH/rs"
    cp -r "$b" "$SCRATCH/ref"; cp -r "$b" "$SCRATCH/rs"
    pcb=$(ls "$SCRATCH/ref"/*.kicad_pcb)
    "$CLI" pcb drc --format report --severity-all --units mm -o "$SCRATCH/ref.rpt" "$pcb" \
        > /dev/null 2>&1
    pcb=$(ls "$SCRATCH/rs"/*.kicad_pcb)
    "$RS" pcb drc --format report --severity-all --units mm -o "$SCRATCH/rs.rpt" "$pcb" \
        > /dev/null 2>&1
    norm "$SCRATCH/ref.rpt" > "$SCRATCH/ref.norm"
    norm "$SCRATCH/rs.rpt" > "$SCRATCH/rs.norm"
    if cmp -s "$SCRATCH/ref.norm" "$SCRATCH/rs.norm"; then
        echo "same   $name"; same=$((same + 1))
    else
        echo "DIFFER $name: $(diff "$SCRATCH/ref.norm" "$SCRATCH/rs.norm" | head -3 | tr '\n' ' ')"
        differ=$((differ + 1))
    fi
done

echo "live2 cli compare: $same identical, $differ different"
rm -rf "$SCRATCH"
