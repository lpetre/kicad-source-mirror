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
# PCBIO-15: reference pcb-dump (QA path and --loader) vs pcb-dump-session over every corpus
# board under 500 KB that the Rust parser loads strictly (--strict); sections board, item,
# track and the rn_net summary of connectivity. One reference process at a time, scratch in
# rust/target/pcbio15 (results.txt: compare.py JSON per path and board; mismatch_keys.txt).
# Build first: cargo build -p kicad-session --features dump --bin pcb-dump-session.
set -u
root=$(cd "$(dirname "$0")/../../../../.." && pwd)
out=$root/rust/target/pcbio15
rs=$root/rust/target/debug/pcb-dump-session
ref=/home/user/kicad-oracle/bin/pcb-dump
S=board,item,track,connectivity
export KICAD_ORACLE_HOME=$out/home; mkdir -p $KICAD_ORACLE_HOME
: > $out/results.txt
find $root/qa/data/pcbnew $root/demos -name '*.kicad_pcb' -size -500k | sort | while read f; do
  stem=$(basename "$f" .kicad_pcb); d=$(dirname "$f")
  mkdir -p $out/work; rm -rf $out/work/*; cp "$f" $out/work/
  for e in kicad_pro kicad_prl kicad_dru pro; do [ -f "$d/$stem.$e" ] && cp "$d/$stem.$e" $out/work/; done
  b=$out/work/$stem.kicad_pcb
  for mode in qa loader; do
    flag=; [ $mode = loader ] && flag=--loader
    timeout 120 $rs board $flag --strict --hash --sections $S "$b" > $out/rs.txt 2>/dev/null
    if grep -q '^error|load' $out/rs.txt; then
      echo "$mode $f NOTLOADABLE $(grep '^error|load' $out/rs.txt | cut -d'|' -f4 | cut -c1-120)" >> $out/results.txt; continue
    fi
    rm -rf $out/work2; mkdir -p $out/work2; cp $out/work/* $out/work2/
    timeout 300 nice -n 10 $ref board $flag --hash --sections $S $out/work2/$stem.kicad_pcb 2>/dev/null \
      | grep -v '^\(cn_item\|cn_cluster\|cn_dangling\|cn_island\)|' | grep -v '^rn_net|.*|\(nodes\|edges\)=' > $out/ref.txt
    sum=$(python3 $root/rust/tools/pcb-dump/compare.py $out/ref.txt $out/rs.txt --summary-json 2>/dev/null | tr -d '\n')
    echo "$mode $f $sum" >> $out/results.txt
    mm=$(python3 $root/rust/tools/pcb-dump/compare.py $out/ref.txt $out/rs.txt --show 1000 2>/dev/null | grep -A0 "^$stem.kicad_pcb|" | cut -d'|' -f2-4)
    [ -n "$mm" ] && echo "$mm" | sed "s|^|$mode $stem |" >> $out/mismatch_keys.txt
  done
done
echo DONE >> $out/results.txt
