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
"""CN-03: writes the synthetic ratsnest boards of tests/golden/rn_random/ (deterministic).

Random via / track point sets that exercise RN_NET: Delaunay triangulation, Filter-Kruskal
recursion (> KRUSKAL_THRESHOLD candidate edges), collinear and near-collinear sets,
coincident anchors of different clusters (chain edges, R-2), single-position nets and nets
with two nodes. gen_rn_random.sh runs the reference pcb-dump on them.
"""
import random
import sys
import os

HEADER = """(kicad_pcb
\t(version 20250901)
\t(generator "pcbnew")
\t(generator_version "9.99")
\t(general
\t\t(thickness 1.6)
\t\t(legacy_teardrops no)
\t)
\t(paper "A4")
\t(layers
\t\t(0 "F.Cu" signal)
\t\t(2 "B.Cu" signal)
\t\t(25 "Edge.Cuts" user)
\t)
\t(setup
\t\t(pad_to_mask_clearance 0)
\t)
"""


class Board:
    def __init__(self, seed, nets):
        self.rng = random.Random(seed)
        self.nets = nets
        self.items = []

    def uuid(self):
        h = "%032x" % self.rng.getrandbits(128)
        return "%s-%s-4%s-8%s-%s" % (h[0:8], h[8:12], h[13:16], h[17:20], h[20:32])

    def via(self, x, y, net, size=0.6):
        self.items.append(
            "\t(via\n\t\t(at %s %s)\n\t\t(size %s)\n\t\t(drill 0.3)\n"
            "\t\t(layers \"F.Cu\" \"B.Cu\")\n\t\t(net %d)\n\t\t(uuid \"%s\")\n\t)\n"
            % (mm(x), mm(y), size, net, self.uuid()))

    def seg(self, x1, y1, x2, y2, layer, net, width=0.2):
        self.items.append(
            "\t(segment\n\t\t(start %s %s)\n\t\t(end %s %s)\n\t\t(width %s)\n"
            "\t\t(layer \"%s\")\n\t\t(net %d)\n\t\t(uuid \"%s\")\n\t)\n"
            % (mm(x1), mm(y1), mm(x2), mm(y2), width, layer, net, self.uuid()))

    def zone(self, pts, layer, net):
        xy = " ".join("(xy %s %s)" % (mm(x), mm(y)) for x, y in pts)
        self.items.append(
            "\t(zone\n\t\t(net %d)\n\t\t(net_name \"N%d\")\n\t\t(layer \"%s\")\n"
            "\t\t(uuid \"%s\")\n\t\t(hatch edge 0.5)\n\t\t(connect_pads\n\t\t\t(clearance 0.2)\n\t\t)\n"
            "\t\t(min_thickness 0.2)\n\t\t(fill yes\n\t\t\t(thermal_gap 0.5)\n"
            "\t\t\t(thermal_bridge_width 0.5)\n\t\t)\n"
            "\t\t(polygon\n\t\t\t(pts\n\t\t\t\t%s\n\t\t\t)\n\t\t)\n"
            "\t\t(filled_polygon\n\t\t\t(layer \"%s\")\n\t\t\t(pts\n\t\t\t\t%s\n\t\t\t)\n\t\t)\n\t)\n"
            % (net, net, layer, self.uuid(), xy, layer, xy))

    def text(self):
        out = HEADER
        out += "\t(net 0 \"\")\n"
        for n in range(1, self.nets + 1):
            out += "\t(net %d \"N%d\")\n" % (n, n)
        out += "".join(self.items)
        out += "\t(embedded_fonts no)\n)\n"
        return out


def mm(nm):
    """nanometres -> exact mm text"""
    s = "-" if nm < 0 else ""
    nm = abs(nm)
    whole, frac = divmod(nm, 1000000)
    if frac == 0:
        return "%s%d" % (s, whole)
    return ("%s%d.%06d" % (s, whole, frac)).rstrip("0")


def dense():
    """~1500 vias of one net on a 0.25 mm grid: many touching clusters, > 1024 edges."""
    b = Board(1, 2)
    for _ in range(1500):
        b.via(b.rng.randrange(0, 240) * 250000, b.rng.randrange(0, 240) * 250000,
              1 + (b.rng.random() < 0.2))
    return b


def collinear():
    """Exactly collinear nets (horizontal, vertical, diagonal) and a net that is collinear but
    for one point."""
    b = Board(2, 4)
    for i in range(40):
        b.via(b.rng.randrange(0, 400) * 1000000 // 7, 5000000, 1)
        b.via(80000000, b.rng.randrange(0, 400) * 1000000 // 3, 2)
        t = b.rng.randrange(0, 300) * 333333
        b.via(t, 2 * t + 1000000, 3)
    for i in range(30):
        t = b.rng.randrange(0, 300) * 400000
        b.via(t + 3, t + 10000000, 4)
    b.via(1000001, 11000000 + 1, 4)
    return b


def near_collinear():
    """Points within a few nanometres of one long line (the delaunator's near-collinear
    handling) on large coordinates."""
    b = Board(3, 3)
    for i in range(60):
        t = b.rng.randrange(0, 1000) * 150000
        b.via(t, t // 3 + b.rng.randrange(-2, 3), 1)
        u = b.rng.randrange(0, 2000) * 90000
        b.via(-u - 1000000, 200000000 - u // 7 + b.rng.randrange(-1, 2), 2)
    for i in range(5):
        b.via(b.rng.randrange(0, 100) * 1000000, 150000000, 3)
    return b


def coincident():
    """F.Cu and B.Cu tracks with ends on a coarse grid: coincident anchors of different
    clusters (chain edges of weight 1, cluster order R-2); single-position nets; two-node
    nets."""
    b = Board(4, 9)
    g = 2000000
    for _ in range(300):
        net = b.rng.choice([1, 1, 2, 3])
        x1, y1 = b.rng.randrange(0, 12) * g, b.rng.randrange(0, 12) * g
        x2, y2 = x1 + b.rng.choice([-1, 0, 1]) * g, y1 + b.rng.choice([-1, 0, 1]) * g
        b.seg(x1, y1, x2, y2, b.rng.choice(["F.Cu", "B.Cu"]), net)
    for _ in range(20):
        b.via(b.rng.randrange(0, 12) * g + g // 2, b.rng.randrange(0, 12) * g + g // 2, 1)
    # one position only: zero-length tracks on both layers
    b.seg(50000000, 50000000, 50000000, 50000000, "F.Cu", 4)
    b.seg(50000000, 50000000, 50000000, 50000000, "B.Cu", 4)
    b.seg(50000000, 50000000, 50000000, 50000000, "F.Cu", 4)
    # one position, overlapping layers only: no edge
    b.via(60000000, 60000000, 5)
    b.seg(60000000, 60000000, 60000000, 60000000, "B.Cu", 5)
    # two nodes
    b.via(70000000, 10000000, 6)
    b.via(75000000, 12000000, 6)
    # two nodes in one cluster
    b.seg(70000000, 20000000, 72000000, 20000000, "F.Cu", 7)
    # three collinear nodes of three clusters
    b.via(70000000, 30000000, 8)
    b.via(71000000, 30000000, 8)
    b.via(72500000, 30000000, 8)
    # single node
    b.via(80000000, 80000000, 9)
    return b


def mixed(seed):
    """Random tracks and vias on five nets."""
    b = Board(seed, 5)
    for _ in range(400):
        net = 1 + b.rng.randrange(5)
        x1, y1 = b.rng.randrange(0, 60000) * 1000, b.rng.randrange(0, 60000) * 1000
        if b.rng.random() < 0.4:
            b.via(x1, y1, net)
        else:
            x2 = x1 + b.rng.randrange(-3000, 3000) * 1000
            y2 = y1 + b.rng.randrange(-3000, 3000) * 1000
            b.seg(x1, y1, x2, y2, b.rng.choice(["F.Cu", "B.Cu"]), net)
    return b


def star(rng, cx, cy, rmin, rmax, n):
    """A star-shaped polygon around ( cx, cy ) with n vertices (counter-clockwise)."""
    import math
    pts = []
    for i in range(n):
        a = 2 * math.pi * i / n
        r = rng.randrange(rmin, rmax)
        pts.append((cx + int(r * math.cos(a)) // 1000 * 1000, cy + int(r * math.sin(a)) // 1000 * 1000))
    return pts


def zones():
    """Filled zones of two nets with vias inside (zone-to-zone anchor optimisation) and lone
    vias / tracks outside (zone anchor optimisation)."""
    b = Board(5, 2)
    centers = []
    for k in range(8):
        cx, cy = b.rng.randrange(5, 95) * 1000000, b.rng.randrange(5, 95) * 1000000
        net = 1 + (k % 2)
        layer = b.rng.choice(["F.Cu", "B.Cu"])
        b.zone(star(b.rng, cx, cy, 3000000, 9000000, b.rng.randrange(8, 40)), layer, net)
        for _ in range(b.rng.randrange(1, 3)):
            b.via(cx + b.rng.randrange(-500, 500) * 1000, cy + b.rng.randrange(-500, 500) * 1000,
                  net)
        centers.append((cx, cy, net))
    for _ in range(12):
        net = 1 + b.rng.randrange(2)
        x, y = b.rng.randrange(0, 100) * 1000000, b.rng.randrange(0, 100) * 1000000
        if b.rng.random() < 0.5:
            b.via(x, y, net)
        else:
            b.seg(x, y, x + 1000000, y, b.rng.choice(["F.Cu", "B.Cu"]), net)
    return b


BOARDS = {
    "rn_zones": zones,
    "rn_dense": dense,
    "rn_collinear": collinear,
    "rn_near_collinear": near_collinear,
    "rn_coincident": coincident,
    "rn_mixed1": lambda: mixed(11),
    "rn_mixed2": lambda: mixed(12),
}

if __name__ == "__main__":
    out = sys.argv[1]
    for name, make in BOARDS.items():
        with open(os.path.join(out, name + ".kicad_pcb"), "w") as f:
            f.write(make().text())
