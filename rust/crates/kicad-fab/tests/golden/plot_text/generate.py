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
"""Regenerates the FAB-06 goldens with the reference kicad-cli (docs/ORACLE.md).

    python3 generate.py

tests/data/plot_text/text.kicad_pcb (hand-written, re-saved with `kicad-cli pcb upgrade
--force`) is exported with `kicad-cli pcb export gerbers` and each option set of OPTION_SETS
on a copy in a scratch dir (the helpers of ../plot_driver/generate.py). Written to
crafted.txt (`S <option set>`, then `<name hash> <header hash> <file hash>` per produced file,
in "Plotted to" order) and the normalised files of the `default` set to crafted_files.txt (`#file <set> <name>`
+ text) for reading differences.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../plot_driver"))
import generate as drv  # noqa: E402

# (name, kicad-cli arguments); tests/plot_text.rs maps each name to the plot options.
OPTION_SETS = [
    ("default", []),
    ("erd", ["--erd"]),
    ("ev", ["--ev"]),
    ("hdnp", ["--hdnp"]),
    ("cdnp", ["--cdnp"]),
]


def main():
    os.makedirs(drv.SCRATCH, exist_ok=True)
    board = os.path.join(drv.CRATE, "tests/data/plot_text/text.kicad_pcb")
    lines = []
    texts = []
    for name, args in OPTION_SETS:
        out = drv.run(board, args, os.path.join(drv.SCRATCH, "w6"))
        lines.append("S %s\n" % name)
        for fname, text in out:
            lines.append("%s %s %s\n" % (drv.fnv(fname)[:8], drv.fnv(drv.header(text))[:8], drv.fnv(text)[:8]))
            if name == "default":  # the other sets: rerun this script (size budget)
                texts.append("#file %s %s\n" % (name, fname))
                texts.append(text)
    with open(os.path.join(HERE, "crafted.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(lines))
    with open(os.path.join(HERE, "crafted_files.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(texts))


if __name__ == "__main__":
    main()
