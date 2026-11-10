#!/usr/bin/env bash
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Zone-fill stage corpus (package ZF-03): compares every intermediate polygon of the copper
# fill pipeline with the reference, using KiCad's own debug mode (`DebugZoneFiller=1` in
# kicad_advanced: the fill of a zone on In<k>.Cu is the k-th DUMP_POLYS_TO_COPPER_LAYER stage
# of its F.Cu fill; In1 smoothed outline, In2 minus thermal reliefs, In3 clearance holes, In4..In6
# spoke test areas, In7 spokes, In8 after spoke trimming, In9 deflated, In10 connected nearby
# polys, In15 re-inflated, In16/In17 trimmed, In18 post-knockout min width, In19 minus
# higher-priority zones; In11..In14 are the full fill for solid zones).
#
# For every 2-layer board of the main corpus (target/zone-corpus, see run.sh) whose zone
# layers are all identical (or the boards given as corpus names), stage_board.py makes a
# 22-layer variant with every F.Cu zone also on In1..In19; the oracle refills it with the debug
# home and the Rust corpus test compares it with ZONE_CORPUS_DEBUG_STAGES=1.
#
# Stages that read other zones' fills (In6, In14, In18, In19 with different-net knockouts) can
# legitimately differ: in debug mode the C++ fill task of In<k>.Cu reads the F.Cu fills of other
# zones without a dependency on them (the DAG is keyed by In<k>.Cu), so the reference depends
# on thread timing.
#
# Usage: tools/zone-corpus/stages.sh [CORPUS_NAME ...]
set -euo pipefail

RUST_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
MAIN="$RUST_DIR/target/zone-corpus"
OUT="$RUST_DIR/target/zone-corpus-stages"
CLI="${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}"
OHOME="$OUT/.oracle-home"

mkdir -p "$OUT" "$OHOME/.config/kicad/10.99"
echo "DebugZoneFiller=1" > "$OHOME/.config/kicad/10.99/kicad_advanced"

if [[ $# -gt 0 ]]; then
    names=("$@")
else
    mapfile -t names < <(grep -E ': zone layers identical [1-9][0-9]*, same area 0, different 0' \
        "$MAIN/report.txt" | cut -d: -f1)
fi

for name in "${names[@]}"; do
    src="$(ls "$MAIN/$name/in/"*.kicad_pcb 2>/dev/null | head -1 || true)"
    [[ -n "$src" ]] || continue
    dir="$OUT/$name"

    if [[ -f "$dir/ref/b.kicad_pcb" ]]; then
        continue
    fi

    rm -rf "$dir"
    mkdir -p "$dir/in" "$dir/ref"

    if ! python3 "$RUST_DIR/tools/zone-corpus/stage_board.py" "$src" "$dir/in/b.kicad_pcb"; then
        rm -rf "$dir" # not a 2-layer board
        continue
    fi

    for ext in kicad_pro kicad_dru; do
        f="$(ls "$MAIN/$name/in/"*.$ext 2>/dev/null | head -1 || true)"
        if [[ -n "$f" ]]; then
            cp "$f" "$dir/in/b.$ext"
        fi
    done

    cp "$dir/in/"b.* "$dir/ref/"

    if ! KICAD_ORACLE_HOME="$OHOME" timeout 600 nice -n 10 "$CLI" pcb drc --refill-zones \
            --save-board -o "$dir/ref/drc.rpt" "$dir/ref/b.kicad_pcb" > "$dir/ref/cli.log" 2>&1 \
            || ! grep -q "Saved board" "$dir/ref/cli.log"; then
        echo "reference failed: $name" >&2
        rm -rf "$dir"
        continue
    fi

    rm -f "$dir/ref/drc.rpt" "$dir/ref/"*.kicad_prl "$dir/ref/"*.lck
done

cd "$RUST_DIR"
ZONE_CORPUS_DIR="$OUT" ZONE_CORPUS_DEBUG_STAGES=1 \
    cargo test -p kicad-zone --release --test zone_fill_corpus -- --nocapture
