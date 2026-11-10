#!/bin/bash
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs kicad-bench (release profile) on the fixed PERF board set and appends the TSV table to
# docs/PERF.md under a heading with the commit hash (PERF-1).
#
#   crates/kicad-bench/scripts/bench.sh [--set small|medium|large|all] [--reps N] [--no-append]
#                                       [--alloc]
#
# The large boards run one at a time with nothing else of ours in parallel (PORTING.md 7);
# never more than one large board is in memory. --alloc also builds the count-alloc variant
# and records allocation counts (separate, slower run).
set -eu

RUST=$(cd "$(dirname "$0")/../../.." && pwd)
KICAD=$(cd "$RUST/.." && pwd)
cd "$RUST"

SET=all
REPS=7
APPEND=1
ALLOC=0
while [ $# -gt 0 ]; do
    case "$1" in
        --set) SET=$2; shift ;;
        --reps) REPS=$2; shift ;;
        --no-append) APPEND=0 ;;
        --alloc) ALLOC=1 ;;
        *) echo "unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done

SMALL="demos/ecc83/ecc83-pp.kicad_pcb qa/data/pcbnew/issue10697.kicad_pcb"
MEDIUM="demos/complex_hierarchy/complex_hierarchy.kicad_pcb demos/interf_u/interf_u.kicad_pcb
        demos/stickhub/StickHub.kicad_pcb
        demos/kit-dev-coldfire-xilinx_5213/kit-dev-coldfire-xilinx_5213.kicad_pcb"
LARGE="demos/vme-wren/vme-wren.kicad_pcb demos/jetson-agx-thor-baseboard/jetson-agx-thor-baseboard.kicad_pcb"

case "$SET" in
    small) BOARDS=$SMALL ;;
    medium) BOARDS=$MEDIUM ;;
    large) BOARDS=$LARGE ;;
    all) BOARDS="$SMALL $MEDIUM $LARGE" ;;
    *) BOARDS=$SET ;;
esac

COMMIT=$(git -C "$KICAD" rev-parse --short HEAD 2>/dev/null || echo unknown)
DIRTY=$(git -C "$KICAD" status --porcelain -- rust/crates 2>/dev/null | head -1)
[ -n "$DIRTY" ] && COMMIT="$COMMIT+dirty"

OUT="$RUST/target/bench"
mkdir -p "$OUT"
TSV="$OUT/bench-$COMMIT-$SET.tsv"

cargo build --release -p kicad-bench
BIN="$RUST/target/release/kicad-bench"
if [ "$ALLOC" = 1 ]; then
    cp "$BIN" "$OUT/kicad-bench-timing"
    cargo build --release -p kicad-bench --features count-alloc
    cp "$BIN" "$OUT/kicad-bench-alloc"
    # BIN is a hard link into target/release/deps: replace the link, never write through it
    # (that would overwrite the count-alloc build cargo then considers up to date).
    cp --remove-destination "$OUT/kicad-bench-timing" "$BIN"
fi

echo "commit	board	stage	n	min_ms	median_ms	max_ms	peak_rss_mb	allocs	alloc_mb" > "$TSV"
for b in $BOARDS; do
    echo "== $b" >&2
    "$BIN" run --board "$KICAD/$b" --reps "$REPS" --warmup 1 --commit "$COMMIT" | tail -n +2 >> "$TSV"
    if [ "$ALLOC" = 1 ]; then
        "$OUT/kicad-bench-alloc" run --board "$KICAD/$b" --reps 1 --warmup 0 --no-mem \
            --commit "$COMMIT" | tail -n +2 | awk -F'\t' -v OFS='\t' '{$3=$3" [alloc]"; print}' >> "$TSV"
    fi
done

echo "wrote $TSV" >&2

if [ "$APPEND" = 1 ]; then
    {
        echo
        echo "### kicad-bench $COMMIT ($SET, $(date -u +%Y-%m-%dT%H:%MZ), $REPS reps, native release)"
        echo
        echo '```tsv'
        cat "$TSV"
        echo '```'
    } >> "$RUST/docs/PERF.md"
    echo "appended to docs/PERF.md" >&2
fi
