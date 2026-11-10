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
# Item-level corpus scoreboard (package STAB-PCB): pcb-dump board --no-init --hash against
# pcb-dump-rs board --hash for every corpus board under 1 MB (qa/data/pcbnew, qa/data/cli,
# demos), one board at a time, classified with compare.py's rules (scoreboard.py).
#
#   scoreboard.sh                     whole corpus; one line per board and section, then the
#                                     summary (also written to $OUT/scoreboard.txt)
#   scoreboard.sh --board PATTERN     only boards whose corpus path contains PATTERN
#   scoreboard.sh --summary           only print the summary of the last results
#   scoreboard.sh --golden            also copy the summary to tests/golden/dump_scoreboard.txt
#
# The reference side is cached (gzip, hashed) under $OUT/ref/, keyed by the SHA-256 of the
# board and of the pcb-dump binary; the Rust side is recomputed on every run. Inputs are
# scratch copies (the tools write side files). Environment: DUMP_CORPUS_DIR (default
# rust/target/dump-corpus), DUMP_SECTIONS, DUMP_MAX_SIZE (bytes, default 1000000).
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
rust=$(cd "$here/../.." && pwd)
root=$(cd "$rust/.." && pwd)
out=${DUMP_CORPUS_DIR:-$rust/target/dump-corpus}
sections=${DUMP_SECTIONS:-item,bbox,track,pad,shape,text,dim,zone,footprint,misc,board,order}
max_size=${DUMP_MAX_SIZE:-1000000}
ref_bin=/home/user/kicad-oracle/bin/pcb-dump
pattern=
mode=all
golden=0

while [ $# -gt 0 ]; do
    case "$1" in
        --board) pattern=${2:?}; shift ;;
        --summary) mode=summary ;;
        --golden) golden=1 ;;
        *) sed -n '19,32p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
    esac
    shift
done

mkdir -p "$out/ref" "$out/res" "$out/home" "$out/logs"

summary() {
    python3 -I "$here/scoreboard.py" summary "$out/res" --classes 200 > "$out/scoreboard.txt"
    cat "$out/scoreboard.txt"

    if [ "$golden" -eq 1 ]; then
        cp "$out/scoreboard.txt" "$rust/crates/kicad-pcb/tests/golden/dump_scoreboard.txt"
    fi
}

if [ "$mode" = summary ]; then
    summary
    exit 0
fi

(cd "$rust" && cargo build -q -p kicad-pcb --features dump --bin pcb-dump-rs) || exit 1
rs_bin=$rust/target/debug/pcb-dump-rs
ref_tag=$(sha256sum "$(readlink -f "$ref_bin")" | cut -c1-16)

cd "$root" || exit 1
mapfile -t boards < <(find qa/data/pcbnew qa/data/cli demos -name '*.kicad_pcb' -size -"$max_size"c \
    | LC_ALL=C sort)

work=$out/work
[ -z "$pattern" ] && rm -f "$out"/res/*.json

for board in "${boards[@]}"; do
    case "$board" in *"$pattern"*) ;; *) continue ;; esac
    flat=$(echo "$board" | sed 's|/|__|g')
    key="$(sha256sum "$board" | cut -c1-16) $ref_tag $sections"
    ref=$out/ref/$flat.txt.gz
    rm -rf "$work"
    mkdir -p "$work"
    file=$work/$(basename "$board")
    cp "$board" "$file"

    if [ ! -f "$ref" ] || [ "$(cat "$out/ref/$flat.key" 2> /dev/null)" != "$key" ]; then
        HOME=$out/home nice -n 15 timeout 600 "$ref_bin" board --no-init --hash \
            --sections "$sections" "$file" 2> "$out/logs/$flat.ref.log" | gzip -c > "$ref.tmp"
        mv "$ref.tmp" "$ref"
        echo "$key" > "$out/ref/$flat.key"
        cp "$board" "$file"
    fi

    nice -n 15 timeout 900 "$rs_bin" board --hash --sections "$sections" "$file" \
        2> "$out/logs/$flat.rust.log" > "$work/rs.txt"
    python3 -I "$here/scoreboard.py" board "$ref" "$work/rs.txt" "$board" "$out/res/$flat.json"
    rm -rf "$work"
done

summary
