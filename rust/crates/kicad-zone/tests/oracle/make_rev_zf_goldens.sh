#!/usr/bin/env bash
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Goldens of the crafted zone-fill boards (package REV-ZF): refills a COPY of every board of
# tests/data/rev_zf/ with the reference `kicad-cli pcb drc --refill-zones --save-board` under
# rust/target/zone-corpus-rev/<name>/{in,ref}/ (the layout of tools/zone-corpus/run.sh, so
# that `ZONE_CORPUS_DIR=target/zone-corpus-rev cargo test -p kicad-zone --release --test
# zone_fill_corpus -- --nocapture` shows the first differing vertex of a board), then writes
# the compact goldens tests/golden/rev_zf/<name>.txt from the reference boards
# (`REV_ZF_BLESS=1`, tests/rev_zf_crafted.rs).
#
# Usage: make_rev_zf_goldens.sh [--regen] [--refs-only]
# Reference runs are sequential (at most one oracle process; PORTING.md section 7).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$(cd "$HERE/../.." && pwd)"
RUST_DIR="$(cd "$CRATE_DIR/../.." && pwd)"
DATA="$CRATE_DIR/tests/data/rev_zf"
OUT="$RUST_DIR/target/zone-corpus-rev"
CLI="${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}"
export KICAD_ORACLE_HOME="${KICAD_ORACLE_HOME:-$OUT/.oracle-home}"

refs_only=0
for arg in "$@"; do
    case "$arg" in
        --regen) python3 "$HERE/gen_rev_zf.py" "$DATA" ;;
        --refs-only) refs_only=1 ;;
    esac
done

mkdir -p "$OUT" "$KICAD_ORACLE_HOME"

for board in "$DATA"/*.kicad_pcb; do
    stem="$(basename "$board" .kicad_pcb)"
    dir="$OUT/$stem"

    # a cached reference is reused while the input is unchanged
    if [[ -f "$dir/ref/$stem.kicad_pcb" ]] && cmp -s "$board" "$dir/in/$stem.kicad_pcb"; then
        continue
    fi

    rm -rf "$dir"
    mkdir -p "$dir/in" "$dir/ref"

    for ext in kicad_pcb kicad_pro kicad_prl kicad_dru; do
        if [[ -f "$DATA/$stem.$ext" ]]; then
            cp "$DATA/$stem.$ext" "$dir/in/"
            cp "$DATA/$stem.$ext" "$dir/ref/"
        fi
    done

    if ! timeout 300 nice -n 10 "$CLI" pcb drc --refill-zones --save-board \
            -o "$dir/ref/drc.rpt" "$dir/ref/$stem.kicad_pcb" > "$dir/ref/cli.log" 2>&1; then
        echo "reference failed: $stem (see $dir/ref/cli.log)" >&2
        rm -f "$dir/ref/$stem.kicad_pcb"
    fi

    rm -f "$dir/ref/"*.lck
done

if [[ $refs_only -eq 0 ]]; then
    cd "$RUST_DIR"
    REV_ZF_BLESS=1 REV_ZF_REF_DIR="$OUT" cargo test -p kicad-zone --release --test rev_zf_crafted -- --nocapture
fi
