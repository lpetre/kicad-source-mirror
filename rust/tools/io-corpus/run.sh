#!/bin/bash
# This program source code file is part of KiCad, a free EDA CAD application.
#
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU General Public License as published by the
# Free Software Foundation, either version 3 of the License, or (at your
# option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
# General Public License for more details.
#
# You should have received a copy of the GNU General Public License along
# with this program.  If not, see <http://www.gnu.org/licenses/>.
#
# Corpus differential harness of the board reader / writer (PCBIO-17). See README.md.
#
#   run.sh                      oracle cache + Rust re-save of every board, scoreboard
#   run.sh --board NAME         one board: report and unified diff O2 vs rust(O1)
#   run.sh --update-golden      full run, then rewrite tests/golden/io_corpus/ (golden set)
#   run.sh --dump NAME [--o1] [--sections LIST] [--hash]
#                               item-level comparison: pcb-dump --no-init vs pcb-dump-rs,
#                               fed to tools/pcb-dump/compare.py
#
# NAME is a corpus path (qa/data/pcbnew/x.kicad_pcb), a file name or a file stem.
# Environment: IO_CORPUS_DIR (default rust/target/io-corpus), IO_CORPUS_JOBS (oracle runs at
# a time, at most 2), DIFF_LINES (default 400), SHOW (compare.py --show, default 10).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
rust=$(cd "$here/../.." && pwd)
root=$(cd "$rust/.." && pwd)
out=${IO_CORPUS_DIR:-$rust/target/io-corpus}
export IO_CORPUS_DIR=$out

mode=all
name=
use_o1=0
sections=
hash=0

usage() {
    sed -n '19,30p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

while [ $# -gt 0 ]; do
    case "$1" in
        --board) mode=board; name=${2:?}; shift ;;
        --dump) mode=dump; name=${2:?}; shift ;;
        --update-golden) mode=golden ;;
        --o1) use_o1=1 ;;
        --sections) sections=${2:?}; shift ;;
        --hash) hash=1 ;;
        -h|--help) usage ;;
        *) usage ;;
    esac
    shift
done

mkdir -p "$out"

run_test() {
    (cd "$rust" && cargo test -q -p kicad-pcb --test io_corpus -- --ignored --exact \
        corpus_scoreboard --nocapture --test-threads 1)
}

case "$mode" in
all)
    rm -f "$out/scoreboard.md"
    run_test
    ;;
golden)
    rm -f "$out/scoreboard.md"
    IO_CORPUS_UPDATE_GOLDEN=1 run_test
    ;;
board)
    rm -f "$out/board/last.env"
    IO_CORPUS_BOARD=$name run_test
    O2= RUST=
    # shellcheck disable=SC1091
    . "$out/board/last.env"
    if [ -n "${RUST:-}" ] && [ -n "${O2:-}" ]; then
        echo "=== diff -u O2 (oracle re-save of O1) rust(O1), first ${DIFF_LINES:-400} lines"
        diff -u --label "O2 $BOARD" --label "rust(O1) $BOARD" "$O2" "$RUST" \
            | head -n "${DIFF_LINES:-400}" || true
        echo "=== files: O1=$O1 O2=$O2 RUST=$RUST"
    fi
    ;;
dump)
    # Resolve NAME to one corpus board.
    cd "$root"
    if [ -f "$name" ]; then
        board=$name
    else
        mapfile -t hits < <(find qa/data/pcbnew demos -name '*.kicad_pcb' \
            \( -path "*/$name" -o -name "$name" -o -name "$name.kicad_pcb" \) | LC_ALL=C sort)
        if [ "${#hits[@]}" -ne 1 ]; then
            echo "dump: '$name' matches ${#hits[@]} boards: ${hits[*]:-}" >&2
            exit 1
        fi
        board=${hits[0]}
    fi
    flat=$(echo "$board" | sed 's|^\./||; s|/|__|g')
    size=$(stat -c %s "$board")
    if [ "$size" -ge 2000000 ] && { [ "$hash" -eq 0 ] || [ -z "$sections" ]; }; then
        echo "dump: $board is $((size / 1000000)) MB: use --hash and a narrow --sections list" >&2
        exit 1
    fi

    work=$out/dump/work
    rm -rf "$work"
    mkdir -p "$work" "$out/dump/home"
    input=$board
    if [ "$use_o1" -eq 1 ]; then
        input=$out/boards/$flat/o1.kicad_pcb
        [ -f "$input" ] || { echo "dump: no cached O1 for $board (run run.sh --board first)" >&2; exit 1; }
    fi
    # Never run the tools on repository files: work on a copy with the original file name.
    cp "$input" "$work/$(basename "$board")"
    file=$work/$(basename "$board")

    args=()
    [ -n "$sections" ] && args+=(--sections "$sections")
    [ "$hash" -eq 1 ] && args+=(--hash)
    ref=$out/dump/$flat.ref.txt
    rs=$out/dump/$flat.rust.txt

    echo "=== pcb-dump board --no-init ${args[*]:-} $board$([ "$use_o1" -eq 1 ] && echo ' (O1)')"
    HOME=$out/dump/home nice -n 15 /home/user/kicad-oracle/bin/pcb-dump board --no-init \
        "${args[@]}" "$file" > "$ref" 2> "$out/dump/$flat.ref.log" || true
    (cd "$rust" && cargo run -q -p kicad-pcb --features dump --bin pcb-dump-rs -- board \
        "${args[@]}" "$file") > "$rs" 2> "$out/dump/$flat.rust.log" || true
    rm -rf "$work"

    echo "=== compare.py (REF = C++, RUST = pcb-dump-rs)"
    python3 -I "$rust/tools/pcb-dump/compare.py" "$ref" "$rs" --show "${SHOW:-10}" || true

    # Keep the dumps for inspection unless they are large.
    for f in "$ref" "$rs"; do
        if [ "$(stat -c %s "$f")" -gt 100000000 ]; then
            rm -f "$f"
            echo "(removed $f: over 100 MB)"
        fi
    done
    echo "=== dumps: $ref $rs"
    ;;
esac

[ "$mode" = all ] || [ "$mode" = golden ] && echo "scoreboard: $out/scoreboard.md"
exit 0
