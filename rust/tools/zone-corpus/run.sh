#!/usr/bin/env bash
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Zone-fill corpus harness (package ZF-01): caches the reference refill of every board
# (`kicad-cli pcb drc --refill-zones --save-board` on a COPY) under rust/target/zone-corpus/,
# then runs the Rust comparison (crates/kicad-zone/tests/zone_fill_corpus.rs), which refills
# the same input with the port and compares every zone layer's fill polygons.
#
# Usage: tools/zone-corpus/run.sh [--refs-only] [BOARD.kicad_pcb ...]
# Without boards: every qa/data board with zones under 500 KB (see list_boards).
#
# Layout per board:  target/zone-corpus/<name>/in/<board>(.kicad_pro, .kicad_dru)
#                    target/zone-corpus/<name>/ref/<board>   (refilled and saved by the oracle)
# Reference runs are sequential (at most one oracle process; machine rules, PORTING.md 7).
set -euo pipefail

RUST_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
KICAD_DIR="$(cd "$RUST_DIR/.." && pwd)"
OUT="$RUST_DIR/target/zone-corpus"
CLI="${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}"
export KICAD_ORACLE_HOME="${KICAD_ORACLE_HOME:-$OUT/.oracle-home}"

refs_only=0
if [[ "${1:-}" == "--refs-only" ]]; then
    refs_only=1
    shift
fi

list_boards() {
    # Boards with zones, < 500 KB, no teardrop regeneration needed is decided by the test.
    find "$KICAD_DIR/qa/data/pcbnew" "$KICAD_DIR/qa/data/cli/artwork_generation_regressions" \
        -name '*.kicad_pcb' -size -500k -print0 2>/dev/null |
        xargs -0 grep -l '(zone' | sort
}

if [[ $# -gt 0 ]]; then
    boards=("$@")
else
    mapfile -t boards < <(list_boards)
fi

mkdir -p "$OUT" "$KICAD_ORACLE_HOME"

for board in "${boards[@]}"; do
    rel="${board#"$KICAD_DIR"/}"
    name="$(echo "$rel" | sed 's#^qa/data/##; s#/#__#g; s#\.kicad_pcb$##')"
    dir="$OUT/$name"
    base="$(basename "$board")"
    stem="${base%.kicad_pcb}"

    if [[ -f "$dir/ref/$base" ]]; then
        continue
    fi

    if [[ ! -x "$CLI" ]]; then
        echo "no oracle kicad-cli at $CLI; skipping reference runs" >&2
        break
    fi

    rm -rf "$dir"
    mkdir -p "$dir/in" "$dir/ref"

    for ext in kicad_pcb kicad_pro kicad_dru; do
        src="$(dirname "$board")/$stem.$ext"
        if [[ -f "$src" ]]; then
            cp "$src" "$dir/in/"
            cp "$src" "$dir/ref/"
        fi
    done

    if ! timeout 600 nice -n 10 "$CLI" pcb drc --refill-zones --save-board \
            -o "$dir/ref/drc.rpt" "$dir/ref/$base" > "$dir/ref/cli.log" 2>&1; then
        # DRC violations do not fail the command without --exit-code-violations.
        echo "reference failed: $rel (see $dir/ref/cli.log)" >&2
        rm -f "$dir/ref/$base"
    fi

    rm -f "$dir/ref/drc.rpt" "$dir/ref/"*.kicad_prl "$dir/ref/"*.lck
done

if [[ $refs_only -eq 0 ]]; then
    cd "$RUST_DIR"
    cargo test -p kicad-zone --release --test zone_fill_corpus -- --nocapture
fi
