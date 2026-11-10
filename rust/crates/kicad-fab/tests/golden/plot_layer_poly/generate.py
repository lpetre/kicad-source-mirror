#!/usr/bin/env python3
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
"""Regenerates the FAB-10 goldens (solder mask minimum width, clipped silk) with the reference
kicad-cli (docs/ORACLE.md).

    python3 generate.py [crafted|corpus|all]

crafted: every board of tests/data/plot_layer_poly x the sets CRAFTED_SETS.
corpus: every .kicad_pcb under qa/data and demos smaller than 300 KB x the sets CORPUS_SETS
  (the CLI defaults, which cover the minimum width on F.Mask / B.Mask, are replayed by
  kicad-session tests/gerbers_job.rs).
Each run of `kicad-cli pcb export gerbers <set> -o out/ <copy of the board>` is written as
`B <set> <board path relative to the KiCad tree>`, then one line per produced file in
"Plotted to" order: `<file name> <hash>`, the hash being the first 8 hex digits of the FNV-1a
64 of the normalised file (the rules of ../plot_driver/generate.py).
Set SCRATCH to a temporary directory (default rust/target/fab-plot-layer-poly-golden).
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../plot_driver"))
import generate as pd  # noqa: E402

pd.SCRATCH = os.environ.get("SCRATCH", os.path.join(pd.KICAD, "rust/target/fab-plot-layer-poly-golden"))

CORPUS_SETS = [
    ("sub", ["--subtract-soldermask", "-l", "F.SilkS,B.SilkS"]),
    ("sub_cl", ["--subtract-soldermask", "-l", "F.Cu,B.Cu", "--cl", "F.SilkS,B.SilkS"]),
]
CRAFTED_SETS = [
    ("default", ["-l", "F.Cu,B.Cu,F.Mask,B.Mask,F.SilkS,B.SilkS"]),
    ("mask_cl", ["-l", "F.Cu,Edge.Cuts", "--cl", "F.Mask,B.Mask"]),
] + CORPUS_SETS


def write(name, boards, sets):
    lines = []
    for rel in boards:
        for set_name, args in sets:
            out = pd.run(os.path.join(pd.KICAD, rel), args, os.path.join(pd.SCRATCH, "w"))
            lines.append("B %s %s\n" % (set_name, rel))
            for fname, text in out:
                lines.append("%s %s\n" % (fname, pd.fnv(text)[:8]))
    with open(os.path.join(HERE, name), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(lines))


def crafted():
    data = os.path.join(pd.CRATE, "tests/data/plot_layer_poly")
    boards = sorted(os.path.relpath(os.path.join(data, n), pd.KICAD)
                    for n in os.listdir(data) if n.endswith(".kicad_pcb"))
    write("crafted.txt", boards, CRAFTED_SETS)


def corpus():
    boards = []
    for top in ("qa/data", "demos"):
        for root, _, files in os.walk(os.path.join(pd.KICAD, top)):
            for f in files:
                p = os.path.join(root, f)
                if f.endswith(".kicad_pcb") and os.path.getsize(p) < 300 * 1024:
                    boards.append(os.path.relpath(p, pd.KICAD))
    write("corpus.txt", sorted(boards), CORPUS_SETS)


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "all"
    os.makedirs(pd.SCRATCH, exist_ok=True)
    if what in ("crafted", "all"):
        crafted()
    if what in ("corpus", "all"):
        corpus()
