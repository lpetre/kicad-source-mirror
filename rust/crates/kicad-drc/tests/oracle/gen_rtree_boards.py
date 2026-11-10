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
"""Writes the synthetic boards of the DRC_RTREE oracle (tests/data/rtree/*.kicad_pcb).

Tracks, arcs and vias only (what the Rust model can shape today), deterministic (seeded),
with deliberate centre ties (Hilbert sort ties), multi-level trees (> 16 leaves per layer) and
blind / buried / micro vias.
"""
import math
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "data", "rtree")

LAYERS2 = [(0, "F.Cu"), (2, "B.Cu")]
LAYERS4 = [(0, "F.Cu"), (4, "In1.Cu"), (6, "In2.Cu"), (2, "B.Cu")]


class Board:
    def __init__(self, copper, seed):
        self.copper = copper
        self.items = []
        self.n = 0
        self.rnd = random.Random(seed)

    def uuid(self):
        self.n += 1
        return "00000000-0000-0000-0000-%012x" % self.n

    def seg(self, x0, y0, x1, y1, w, layer):
        self.items.append(
            f'\t(segment\n\t\t(start {x0:g} {y0:g})\n\t\t(end {x1:g} {y1:g})\n\t\t(width {w:g})\n'
            f'\t\t(layer "{layer}")\n\t\t(net "")\n\t\t(uuid "{self.uuid()}")\n\t)\n')

    def arc(self, cx, cy, r, a0, a1, w, layer):
        def pt(a):
            return (round(cx + r * math.cos(math.radians(a)), 6),
                    round(cy + r * math.sin(math.radians(a)), 6))
        s, m, e = pt(a0), pt((a0 + a1) / 2), pt(a1)
        self.items.append(
            f'\t(arc\n\t\t(start {s[0]:g} {s[1]:g})\n\t\t(mid {m[0]:g} {m[1]:g})\n'
            f'\t\t(end {e[0]:g} {e[1]:g})\n\t\t(width {w:g})\n\t\t(layer "{layer}")\n'
            f'\t\t(net "")\n\t\t(uuid "{self.uuid()}")\n\t)\n')

    def via(self, x, y, size, drill, top, bot, kind=""):
        k = (" " + kind) if kind else ""
        self.items.append(
            f'\t(via{k}\n\t\t(at {x:g} {y:g})\n\t\t(size {size:g})\n\t\t(drill {drill:g})\n'
            f'\t\t(layers "{top}" "{bot}")\n\t\t(net "")\n\t\t(uuid "{self.uuid()}")\n\t)\n')

    def write(self, name):
        layers = "".join(f'\t\t({i} "{n}" signal)\n' for i, n in self.copper)
        text = ('(kicad_pcb\n\t(version 20260819)\n\t(generator "pcbnew")\n'
                '\t(generator_version "10.99")\n\t(general\n\t\t(thickness 1.6)\n'
                '\t\t(legacy_teardrops no)\n\t)\n\t(paper "A4")\n\t(layers\n' + layers +
                '\t\t(25 "Edge.Cuts" user)\n\t)\n' + "".join(self.items) + ')\n')
        with open(os.path.join(OUT, name), "w") as f:
            f.write(text)


def small():
    b = Board(LAYERS2, 1)
    # crossing and touching tracks
    b.seg(100, 100, 110, 100, 0.25, "F.Cu")
    b.seg(105, 95, 105, 105, 0.25, "F.Cu")
    b.seg(110, 100, 115, 103, 0.3, "F.Cu")
    b.seg(100, 100.4, 110, 100.4, 0.2, "F.Cu")
    b.seg(100, 101, 110, 101, 0.2, "B.Cu")
    b.seg(100, 101, 110, 101, 0.2, "F.Cu")
    # arcs
    b.arc(105, 100, 3, 0, 90, 0.25, "F.Cu")
    b.arc(105, 100, 3.3, 10, 170, 0.2, "B.Cu")
    b.arc(120, 110, 2, -45, 200, 0.3, "F.Cu")
    # vias, two at the same position (tie in the Hilbert sort)
    b.via(105, 100, 0.6, 0.3, "F.Cu", "B.Cu")
    b.via(105, 100, 0.8, 0.4, "F.Cu", "B.Cu")
    b.via(110, 100, 0.6, 0.3, "F.Cu", "B.Cu")
    b.via(130, 130, 0.6, 0.3, "F.Cu", "B.Cu")
    b.seg(129, 130, 131, 130, 0.25, "B.Cu")
    # a zero-length track and a far-away loner
    b.seg(140, 140, 140, 140, 0.25, "F.Cu")
    b.seg(160, 100, 165, 100, 0.25, "B.Cu")
    b.write("rtree_small.kicad_pcb")


def four_layer():
    b = Board(LAYERS4, 2)
    names = [n for _, n in LAYERS4]
    r = b.rnd
    for _ in range(90):
        x0, y0 = r.randint(0, 400) / 10 + 100, r.randint(0, 300) / 10 + 100
        ang = r.choice([0, 45, 90, 135, 30])
        ln = r.randint(1, 80) / 10
        x1 = round(x0 + ln * math.cos(math.radians(ang)), 4)
        y1 = round(y0 + ln * math.sin(math.radians(ang)), 4)
        b.seg(x0, y0, x1, y1, r.choice([0.15, 0.2, 0.25, 0.5]), r.choice(names))
    for _ in range(20):
        b.arc(r.randint(0, 400) / 10 + 100, r.randint(0, 300) / 10 + 100, r.randint(5, 50) / 10,
              r.randint(0, 180), r.randint(190, 350), r.choice([0.2, 0.3]), r.choice(names))
    for _ in range(30):
        x, y = r.randint(0, 400) / 10 + 100, r.randint(0, 300) / 10 + 100
        kind = r.choice(["", "", "blind", "micro"])
        if kind == "":
            b.via(x, y, 0.6, 0.3, "F.Cu", "B.Cu")
        elif kind == "blind":
            b.via(x, y, 0.5, 0.25, "F.Cu", "In2.Cu", "blind")
        else:
            b.via(x, y, 0.3, 0.1, "F.Cu", "In1.Cu", "micro")
    b.write("rtree_4layer.kicad_pcb")


def ties():
    b = Board(LAYERS2, 3)
    # 24 vias and 24 tracks sharing one centre, plus a grid around (introsort beyond the
    # 16-element insertion sort threshold with many equal keys)
    for i in range(24):
        b.via(120, 120, 0.4 + 0.01 * i, 0.2, "F.Cu", "B.Cu")
        d = 0.5 + 0.1 * i
        b.seg(120 - d, 120, 120 + d, 120, 0.2, "F.Cu" if i % 2 else "B.Cu")
    for ix in range(6):
        for iy in range(6):
            b.seg(110 + 4 * ix, 110 + 4 * iy, 111 + 4 * ix, 110 + 4 * iy, 0.2,
                  "F.Cu" if (ix + iy) % 2 else "B.Cu")
    b.write("rtree_ties.kicad_pcb")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    small()
    four_layer()
    ties()
