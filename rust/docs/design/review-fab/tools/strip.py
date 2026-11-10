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
#
# REV-FAB review harness (rust/docs/design/review-fab.md). Scratch output goes to
# rust/target/rev-fab/work; the reference kicad-cli always runs on copies.
"""strip.py BOARD OUTDIR TAG kind[,kind...] [keep_only_index_spec]: drop top-level blocks of the given kinds."""
import os, re, shutil, sys
board, outdir, tag, kinds = sys.argv[1:5]
kinds = set(kinds.split(","))
stem = os.path.splitext(os.path.basename(board))[0]
lines = open(board, encoding="utf-8").read().split("\n")
out = []; skip = False; n = 0
for l in lines:
    m = re.match(r"^\t\(([a-z_0-9]+)", l)
    if m and not skip:
        if m.group(1) in kinds:
            n += 1
            if not l.rstrip().endswith(")") or l.count("(") != l.count(")"):
                skip = True
            continue
    if skip:
        if l == "\t)": skip = False
        continue
    out.append(l)
d = os.path.join(outdir, tag); os.makedirs(d, exist_ok=True)
open(os.path.join(d, stem + ".kicad_pcb"), "w", encoding="utf-8").write("\n".join(out))
for ext in (".kicad_pro", ".kicad_dru"):
    p = os.path.join(os.path.dirname(board), stem + ext)
    if os.path.exists(p): shutil.copy(p, d)
print("removed", n, "->", os.path.join(d, stem + ".kicad_pcb"))
