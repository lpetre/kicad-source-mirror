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
"""Regenerates the FAB-05 goldens with the reference kicad-cli (docs/ORACLE.md).

    python3 generate.py

Every board of tests/data/plot_graphics (hand-written, re-saved with `kicad-cli pcb upgrade
--force`), `kicad-cli pcb export gerbers` with the CLI defaults (`--board-plot-params` for the
boards named `bp_*`) on a copy in a scratch dir (the helpers of ../plot_driver/generate.py).
Written to crafted.txt in the compact corpus format of ../plot_driver/corpus.txt
(`B <path relative to the crate>`, then `<name hash> <header hash> <file hash>` per produced
file), and the normalised files to crafted_files.txt (`#file <name>` + text) for reading
differences.

SVG: see `svg()`.
"""
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../plot_driver"))
import generate as drv  # noqa: E402


def main():
    os.makedirs(drv.SCRATCH, exist_ok=True)
    data = os.path.join(drv.CRATE, "tests/data/plot_graphics")
    lines = []
    texts = []
    for name in sorted(os.listdir(data)):
        if not name.endswith(".kicad_pcb"):
            continue
        args = ["--board-plot-params"] if name.startswith("bp_") else []
        out = drv.run(os.path.join(data, name), args, os.path.join(drv.SCRATCH, "w5"))
        lines.append("B tests/data/plot_graphics/%s\n" % name)
        for fname, text in out:
            lines.append("%s %s %s\n" % (drv.fnv(fname)[:8], drv.fnv(drv.header(text))[:8], drv.fnv(text)[:8]))
            texts.append("#file %s\n" % fname)
            texts.append(text)
    with open(os.path.join(HERE, "crafted.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(lines))
    with open(os.path.join(HERE, "crafted_files.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(texts))


SVG_CASES = [
    ("f_silks", ["-l", "F.Silkscreen"]),
    ("dwgs_mirror", ["-l", "User.Drawings", "-m"]),
    ("f_mask", ["-l", "F.Mask"]),
]


def svg():
    """`kicad-cli pcb export svg --mode-single --exclude-drawing-sheet --drill-shape-opt 0
    --black-and-white -l <layer> [-m]` on graphics.kicad_pcb, stored as svg_<case>.svg."""
    board = os.path.join(drv.CRATE, "tests/data/plot_graphics/graphics.kicad_pcb")
    work = os.path.join(drv.SCRATCH, "svg5")
    for case, args in SVG_CASES:
        shutil.rmtree(work, ignore_errors=True)
        os.makedirs(work)
        shutil.copy(board, work)
        env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(drv.SCRATCH, "home"))
        subprocess.run([drv.CLI, "pcb", "export", "svg", "--mode-single", "--exclude-drawing-sheet",
                        "--drill-shape-opt", "0", "--black-and-white", *args,
                        "-o", os.path.join(work, "a.svg"), os.path.join(work, "graphics.kicad_pcb")],
                       env=env, capture_output=True, text=True, timeout=600, check=True)
        shutil.copy(os.path.join(work, "a.svg"), os.path.join(HERE, "svg_%s.svg" % case))
    shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
    svg()
