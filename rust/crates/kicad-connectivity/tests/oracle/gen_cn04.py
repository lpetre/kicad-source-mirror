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
"""CN-04: writes the synthetic CONNECTIVITY_DATA boards of tests/golden/cn04/ (deterministic).

Four copper layers. Hand-placed cases for IsConnectedOnLayer through PCB_VIA::FlashLayer
(conditionally flashed vias touching tracks, arcs and other vias on some layers only, zones
that do not count, nets that do not match) and for TestTrackEndpointDangling (free ends, ends
on vias, short tracks inside one via, tracks inside a zone fill, vias connected on one or two
layers, net-less vias), plus a random part. gen_cn04.sh runs the reference pcb-dump on them.
"""
import os
import random
import sys

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
\t\t(4 "In1.Cu" signal)
\t\t(6 "In2.Cu" signal)
\t\t(2 "B.Cu" signal)
\t\t(25 "Edge.Cuts" user)
\t)
\t(setup
\t\t(pad_to_mask_clearance 0)
\t)
"""

LAYERS = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]

# via flashing modes: (remove_unused_layers, keep_end_layers, start_end_only) flags
KEEP_ALL = ""
EXCEPT_ENDS = "\t\t(remove_unused_layers yes)\n\t\t(keep_end_layers yes)\n"
REMOVE_ALL = "\t\t(remove_unused_layers yes)\n\t\t(keep_end_layers no)\n"


class Board:
    def __init__(self, seed, nets):
        self.rng = random.Random(seed)
        self.nets = nets
        self.items = []

    def uuid(self):
        h = "%032x" % self.rng.getrandbits(128)
        return "%s-%s-4%s-8%s-%s" % (h[0:8], h[8:12], h[13:16], h[17:20], h[20:32])

    def via(self, x, y, net, mode=KEEP_ALL, top="F.Cu", bot="B.Cu", size=0.6, kind=""):
        kind = (" " + kind) if kind else ""
        self.items.append(
            "\t(via%s\n\t\t(at %s %s)\n\t\t(size %s)\n\t\t(drill 0.3)\n"
            "\t\t(layers \"%s\" \"%s\")\n%s\t\t(net %d)\n\t\t(uuid \"%s\")\n\t)\n"
            % (kind, mm(x), mm(y), size, top, bot, mode, net, self.uuid()))

    def seg(self, x1, y1, x2, y2, layer, net, width=0.2):
        self.items.append(
            "\t(segment\n\t\t(start %s %s)\n\t\t(end %s %s)\n\t\t(width %s)\n"
            "\t\t(layer \"%s\")\n\t\t(net %d)\n\t\t(uuid \"%s\")\n\t)\n"
            % (mm(x1), mm(y1), mm(x2), mm(y2), width, layer, net, self.uuid()))

    def arc(self, x1, y1, xm, ym, x2, y2, layer, net, width=0.2):
        self.items.append(
            "\t(arc\n\t\t(start %s %s)\n\t\t(mid %s %s)\n\t\t(end %s %s)\n\t\t(width %s)\n"
            "\t\t(layer \"%s\")\n\t\t(net %d)\n\t\t(uuid \"%s\")\n\t)\n"
            % (mm(x1), mm(y1), mm(xm), mm(ym), mm(x2), mm(y2), width, layer, net, self.uuid()))

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


MM = 1000000


def flash():
    """Conditionally flashed vias and what touches them, per layer."""
    b = Board(11, 3)
    y = 10 * MM
    # through via, ends kept, track on In1 only
    b.via(10 * MM, y, 1, EXCEPT_ENDS)
    b.seg(10 * MM, y, 20 * MM, y, "In1.Cu", 1)
    # through via, all removed, track on F.Cu
    b.via(30 * MM, y, 1, REMOVE_ALL)
    b.seg(30 * MM, y, 35 * MM, y + 3 * MM, "F.Cu", 1)
    # two overlapping vias of one net: the blind one spans F.Cu..In1 only
    b.via(50 * MM, y, 1, REMOVE_ALL)
    b.via(50 * MM + 300000, y, 1, REMOVE_ALL, "F.Cu", "In1.Cu", kind="blind")
    # a track of another net on In2 crossing a via (no contact: different nets)
    b.via(70 * MM, y, 1, REMOVE_ALL)
    b.seg(65 * MM, y, 75 * MM, y, "In2.Cu", 2)
    # a track ending just short of the ring on In1 (no contact) and one just touching it
    b.via(90 * MM, y, 1, REMOVE_ALL)
    b.seg(90 * MM + 400000, y, 95 * MM, y, "In1.Cu", 1)
    b.seg(90 * MM - 300000 - 100000, y, 85 * MM, y, "In2.Cu", 1)
    # net-less via and net-less track on In1
    b.via(110 * MM, y, 0, REMOVE_ALL)
    b.seg(110 * MM, y, 115 * MM, y, "In1.Cu", 0)
    # a via inside a filled zone on In1 (zones are not among the flashing types)
    b.zone([(125 * MM, 5 * MM), (135 * MM, 5 * MM), (135 * MM, 15 * MM), (125 * MM, 15 * MM)],
           "In1.Cu", 1)
    b.via(130 * MM, y, 1, REMOVE_ALL)
    b.seg(130 * MM, y, 130 * MM, y + 2 * MM, "B.Cu", 1)
    # an arc on In2 ending on a via
    b.via(150 * MM, y, 1, EXCEPT_ENDS)
    b.arc(150 * MM, y, 152 * MM, y + 2 * MM, 154 * MM, y, "In2.Cu", 1)
    # blind via F.Cu..In1 on a buried via In1..In2 (ends kept on the blind, none on the buried)
    b.via(170 * MM, y, 2, EXCEPT_ENDS, "F.Cu", "In1.Cu", kind="blind")
    b.via(170 * MM, y, 2, REMOVE_ALL, "In1.Cu", "In2.Cu", kind="buried")
    b.seg(170 * MM, y, 175 * MM, y, "F.Cu", 2)
    # through vias of different sizes overlapping; one keeps all layers
    b.via(190 * MM, y, 3, REMOVE_ALL, size=1.0)
    b.via(190 * MM + 700000, y, 3, KEEP_ALL, size=0.6)
    b.via(190 * MM + 1500000, y, 3, REMOVE_ALL, size=0.6)
    return b


def dangling():
    """TestTrackEndpointDangling cases."""
    b = Board(12, 3)
    y = 40 * MM
    # free track
    b.seg(10 * MM, y, 15 * MM, y, "F.Cu", 1)
    # track with one end on a via, the other free
    b.via(20 * MM, y, 1)
    b.seg(20 * MM, y, 25 * MM, y + 1 * MM, "F.Cu", 1)
    # short track with both ends inside one via
    b.via(30 * MM, y, 1, size=1.0)
    b.seg(30 * MM - 100000, y, 30 * MM + 150000, y, "F.Cu", 1)
    # short track with both ends inside one via and a second item at one end
    b.via(35 * MM, y, 1, size=1.0)
    b.seg(35 * MM - 100000, y, 35 * MM + 150000, y, "B.Cu", 1)
    b.seg(35 * MM + 150000, y, 38 * MM, y, "B.Cu", 1)
    # chain of tracks between two vias (nothing dangles)
    b.via(40 * MM, y, 2)
    b.seg(40 * MM, y, 42 * MM, y, "In1.Cu", 2)
    b.seg(42 * MM, y, 44 * MM, y + 1 * MM, "In1.Cu", 2)
    b.seg(44 * MM, y + 1 * MM, 46 * MM, y, "In1.Cu", 2)
    b.via(46 * MM, y, 2)
    # track inside a zone fill (both ends) and one leaving it
    b.zone([(50 * MM, 35 * MM), (60 * MM, 35 * MM), (60 * MM, 45 * MM), (50 * MM, 45 * MM)],
           "B.Cu", 1)
    b.seg(52 * MM, y, 55 * MM, y, "B.Cu", 1)
    b.seg(58 * MM, y, 64 * MM, y, "B.Cu", 1)
    # vias: unconnected with a net / without, connected on one layer, on two layers
    b.via(70 * MM, y, 1)
    b.via(72 * MM, y, 0)
    b.via(74 * MM, y, 3)
    b.seg(74 * MM, y, 74 * MM, y + 3 * MM, "F.Cu", 3)
    b.seg(74 * MM, y, 76 * MM, y + 3 * MM, "F.Cu", 3)
    b.via(78 * MM, y, 3)
    b.seg(78 * MM, y, 78 * MM, y + 3 * MM, "F.Cu", 3)
    b.seg(78 * MM, y, 80 * MM, y + 3 * MM, "In2.Cu", 3)
    # arcs: free, and with both ends on vias
    b.arc(90 * MM, y, 92 * MM, y + 2 * MM, 94 * MM, y, "F.Cu", 2)
    b.via(100 * MM, y, 2)
    b.via(104 * MM, y, 2)
    b.arc(100 * MM, y, 102 * MM, y - 2 * MM, 104 * MM, y, "B.Cu", 2)
    # zero-length track on a via, and alone
    b.via(110 * MM, y, 1)
    b.seg(110 * MM, y, 110 * MM, y, "F.Cu", 1)
    b.seg(115 * MM, y, 115 * MM, y, "F.Cu", 1)
    # tracks crossing in the middle only (no end contact)
    b.seg(120 * MM, y - 2 * MM, 120 * MM, y + 2 * MM, "F.Cu", 1)
    b.seg(118 * MM, y, 122 * MM, y, "F.Cu", 1)
    return b


def random_board(seed):
    """Random tracks and vias (some conditionally flashed) on all four layers."""
    b = Board(seed, 4)
    modes = [KEEP_ALL, EXCEPT_ENDS, REMOVE_ALL]
    for _ in range(250):
        net = b.rng.randrange(0, 5)
        x1 = b.rng.randrange(0, 200) * 250000
        y1 = b.rng.randrange(0, 200) * 250000
        if b.rng.random() < 0.4:
            top, bot = "F.Cu", "B.Cu"
            kind = ""
            r = b.rng.random()
            if r < 0.2:
                top, bot, kind = "F.Cu", "In1.Cu", "blind"
            elif r < 0.3:
                top, bot, kind = "In1.Cu", "In2.Cu", "buried"
            b.via(x1, y1, net, b.rng.choice(modes), top, bot, kind=kind)
        else:
            x2 = x1 + b.rng.randrange(-12, 13) * 250000
            y2 = y1 + b.rng.randrange(-12, 13) * 250000
            b.seg(x1, y1, x2, y2, b.rng.choice(LAYERS), net)
    return b


BOARDS = {
    "cn04_flash": flash,
    "cn04_dangling": dangling,
    "cn04_random1": lambda: random_board(21),
    "cn04_random2": lambda: random_board(22),
}

if __name__ == "__main__":
    out = sys.argv[1]
    for name, make in BOARDS.items():
        with open(os.path.join(out, name + ".kicad_pcb"), "w") as f:
            f.write(make().text())
