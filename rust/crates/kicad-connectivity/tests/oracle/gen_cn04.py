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

REV-CN added the rev_* boards: zone contact paths, the deferred via-net rule, islands, a
teardrop zone, the ratsnest next to zones, copper graphics, net propagation without pads and
the results that follow the order of CN_ITEM::m_connected
(rust/docs/design/review-connectivity.md).
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
        self.shapes = []

    def uuid(self):
        h = "%032x" % self.rng.getrandbits(128)
        return "%s-%s-4%s-8%s-%s" % (h[0:8], h[8:12], h[13:16], h[17:20], h[20:32])

    def via(self, x, y, net, mode=KEEP_ALL, top="F.Cu", bot="B.Cu", size=0.6, kind="", free=False):
        kind = (" " + kind) if kind else ""
        free = "\t\t(free yes)\n" if free else ""
        self.items.append(
            "\t(via%s\n\t\t(at %s %s)\n\t\t(size %s)\n\t\t(drill 0.3)\n"
            "\t\t(layers \"%s\" \"%s\")\n%s%s\t\t(net %d)\n\t\t(uuid \"%s\")\n\t)\n"
            % (kind, mm(x), mm(y), size, top, bot, mode, free, net, self.uuid()))

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

    def zone_ex(self, outline, fills, net, teardrop=False):
        """A zone with explicit fills: `fills` is a list of (layer, pts), one per fill outline
        (several per layer = islands); the zone is on every layer named there."""
        def xy(pts):
            return " ".join("(xy %s %s)" % (mm(x), mm(y)) for x, y in pts)

        layers = []
        for layer, _ in fills:
            if layer not in layers:
                layers.append(layer)
        if len(layers) == 1:
            layer_text = "\t\t(layer \"%s\")\n" % layers[0]
        else:
            layer_text = "\t\t(layers %s)\n" % " ".join("\"%s\"" % l for l in layers)
        attr = ""
        if teardrop:
            attr = ("\t\t(name \"$teardrop_padvia$\")\n\t\t(hatch full 0.1)\n\t\t(priority 30000)\n"
                    "\t\t(attr\n\t\t\t(teardrop\n\t\t\t\t(type padvia)\n\t\t\t)\n\t\t)\n")
        else:
            attr = "\t\t(hatch edge 0.5)\n"
        out = ("\t(zone\n\t\t(net %d)\n\t\t(net_name \"%s\")\n%s\t\t(uuid \"%s\")\n%s"
               "\t\t(connect_pads\n\t\t\t(clearance 0.2)\n\t\t)\n"
               "\t\t(min_thickness 0.2)\n\t\t(fill yes\n\t\t\t(thermal_gap 0.5)\n"
               "\t\t\t(thermal_bridge_width 0.5)\n\t\t)\n"
               "\t\t(polygon\n\t\t\t(pts\n\t\t\t\t%s\n\t\t\t)\n\t\t)\n"
               % (net, ("N%d" % net) if net else "", layer_text, self.uuid(), attr, xy(outline)))
        for layer, pts in fills:
            out += ("\t\t(filled_polygon\n\t\t\t(layer \"%s\")\n\t\t\t(pts\n\t\t\t\t%s\n"
                    "\t\t\t)\n\t\t)\n" % (layer, xy(pts)))
        self.items.append(out + "\t)\n")

    def gr(self, kind, body, layer, net, width=0.2, fill=None):
        """A board graphic (`gr_line`, `gr_arc`, `gr_circle`, `gr_rect`, `gr_poly`)."""
        fill_text = "" if fill is None else "\t\t(fill %s)\n" % fill
        net_text = "" if net is None else "\t\t(net %d)\n" % net
        self.shapes.append(
            "\t(%s\n%s\t\t(stroke\n\t\t\t(width %s)\n\t\t\t(type solid)\n\t\t)\n%s"
            "\t\t(layer \"%s\")\n%s\t\t(uuid \"%s\")\n\t)\n"
            % (kind, body, width, fill_text, layer, net_text, self.uuid()))

    def gr_line(self, x1, y1, x2, y2, layer, net, width=0.2):
        self.gr("gr_line", "\t\t(start %s %s)\n\t\t(end %s %s)\n" % (mm(x1), mm(y1), mm(x2), mm(y2)),
                layer, net, width)

    def gr_arc(self, x1, y1, xm, ym, x2, y2, layer, net, width=0.2):
        self.gr("gr_arc", "\t\t(start %s %s)\n\t\t(mid %s %s)\n\t\t(end %s %s)\n"
                % (mm(x1), mm(y1), mm(xm), mm(ym), mm(x2), mm(y2)), layer, net, width)

    def gr_circle(self, cx, cy, r, layer, net, width=0.2, fill="no"):
        self.gr("gr_circle", "\t\t(center %s %s)\n\t\t(end %s %s)\n" % (mm(cx), mm(cy), mm(cx + r), mm(cy)),
                layer, net, width, fill)

    def gr_rect(self, x1, y1, x2, y2, layer, net, width=0.2, fill="no"):
        self.gr("gr_rect", "\t\t(start %s %s)\n\t\t(end %s %s)\n" % (mm(x1), mm(y1), mm(x2), mm(y2)),
                layer, net, width, fill)

    def gr_poly(self, pts, layer, net, width=0.2, fill="yes"):
        xy = " ".join("(xy %s %s)" % (mm(x), mm(y)) for x, y in pts)
        self.gr("gr_poly", "\t\t(pts\n\t\t\t%s\n\t\t)\n" % xy, layer, net, width, fill)

    def text(self):
        # File order of pcbnew: graphics, then tracks, then zones.
        tracks = [i for i in self.items if not i.startswith("\t(zone")]
        zones = [i for i in self.items if i.startswith("\t(zone")]
        if self.shapes:
            self.items = self.shapes + tracks + zones
            self.shapes = []
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


def rect(x1, y1, x2, y2):
    return [(x1 * MM, y1 * MM), (x2 * MM, y1 * MM), (x2 * MM, y2 * MM), (x1 * MM, y2 * MM)]


def rev_zone_contacts():
    """REV-CN: the zone contact paths of CN_VISITOR (zone / item by anchor and by shape, zone /
    zone), the deferred via-net rule, islands and a teardrop zone."""
    b = Board(31, 4)
    y = 10 * MM
    # --- zone / item: zone N1 on F.Cu
    z = rect(10, 5, 20, 15)
    b.zone_ex(z, [("F.Cu", z)], 1)
    b.seg(12 * MM, y, 25 * MM, y, "F.Cu", 1)                 # start anchor inside
    b.seg(8 * MM, 7 * MM, 22 * MM, 7 * MM, "F.Cu", 1)        # crosses, both ends outside
    b.seg(20 * MM + 100000, 12 * MM, 25 * MM, 12 * MM, "F.Cu", 1)   # cap just touches the edge
    b.seg(20 * MM + 100001, 13 * MM, 25 * MM, 13 * MM, "F.Cu", 1)   # 1 nm short
    b.seg(12 * MM, 11 * MM, 18 * MM, 11 * MM, "B.Cu", 1)     # other layer
    b.seg(12 * MM, 9 * MM, 18 * MM, 9 * MM, "F.Cu", 2)       # other net (a track may change net)
    b.via(15 * MM, 12 * MM, 1)                               # anchor inside
    b.via(20 * MM + 200000, 14 * MM, 1)                      # only the ring overlaps
    b.via(20 * MM + 300000, 6 * MM, 1)                       # ring touches the edge exactly
    b.via(17 * MM, 6 * MM, 1, REMOVE_ALL)                    # conditionally flashed, inside
    b.via(13 * MM, 13 * MM, 1, KEEP_ALL, "F.Cu", "In1.Cu", kind="blind")
    b.via(11 * MM, 6 * MM, 1, KEEP_ALL, "In1.Cu", "In2.Cu", kind="buried")   # not on F.Cu
    b.arc(9 * MM, 13 * MM, 10 * MM + 500000, 14 * MM, 9 * MM, 15 * MM, "F.Cu", 1)   # body only
    # --- the deferred via-net rule: zones N2 on In1 and N3 on In2 over each other
    z = rect(40, 5, 50, 15)
    b.zone_ex(z, [("In1.Cu", z)], 2)
    b.zone_ex(z, [("In2.Cu", z)], 3)
    b.via(42 * MM, 8 * MM, 0)                                # no net: takes the lowest (2)
    b.via(44 * MM, 8 * MM, 3)                                # its net matches a zone: kept
    b.via(46 * MM, 8 * MM, 1)                                # matches none: lowest (2)
    b.via(48 * MM, 8 * MM, 1)                                # has a track: untouched
    b.seg(48 * MM, 8 * MM, 48 * MM, 3 * MM, "F.Cu", 1)
    b.via(42 * MM, 12 * MM, 4, free=True)                    # free via of another net: no contact
    b.via(44 * MM, 12 * MM, 2, free=True)                    # free via of the In1 zone's net
    b.via(46 * MM, 12 * MM, 0, KEEP_ALL, "F.Cu", "In1.Cu", kind="blind")    # touches N2 only
    b.via(48 * MM, 12 * MM, 0)                               # net-less with a net-less track
    b.seg(48 * MM, 12 * MM, 52 * MM, 17 * MM, "B.Cu", 0)
    b.via(41 * MM, 14 * MM, 4)                               # track of another net wins later
    b.seg(41 * MM, 14 * MM, 38 * MM, 14 * MM, "F.Cu", 1)
    # --- zone / zone on B.Cu
    za = rect(70, 5, 80, 15)
    b.zone_ex(za, [("B.Cu", za)], 1)
    zb = rect(78, 8, 88, 12)
    b.zone_ex(zb, [("B.Cu", zb)], 1)                         # corners inside A
    zc = rect(90, 5, 95, 10)
    zd = rect(95, 10, 100, 15)
    b.zone_ex(zc, [("B.Cu", zc)], 1)                         # C and D share one corner
    b.zone_ex(zd, [("B.Cu", zd)], 1)
    ze = rect(114, 2, 116, 18)
    zf = rect(110, 9, 120, 11)
    b.zone_ex(ze, [("B.Cu", ze)], 1)                         # a cross: no corner inside the other
    b.zone_ex(zf, [("B.Cu", zf)], 1)
    zg = rect(72, 13, 76, 17)
    b.zone_ex(zg, [("B.Cu", zg)], 2)                         # other net over A
    zh = rect(71, 6, 75, 10)
    b.zone_ex(zh, [("F.Cu", zh)], 1)                         # other layer over A
    zs = rect(80, 20, 85, 25)
    zt = rect(85, 20, 90, 25)
    b.zone_ex(zs, [("B.Cu", zs)], 1)                         # S and T share an edge
    b.zone_ex(zt, [("B.Cu", zt)], 1)
    # --- islands: one zone, three fill outlines on B.Cu, a two-layer zone
    zi = rect(130, 5, 160, 15)
    b.zone_ex(zi, [("B.Cu", rect(130, 5, 138, 15)), ("B.Cu", rect(142, 5, 150, 15)),
                   ("B.Cu", rect(152, 5, 156, 9))], 1)
    b.seg(136 * MM, y, 144 * MM, y, "B.Cu", 1)               # bridges the first two islands
    b.via(132 * MM, 7 * MM, 1)                               # second connection of the first
    zj = rect(130, 20, 150, 30)
    b.zone_ex(zj, [("F.Cu", rect(130, 20, 150, 30)), ("In1.Cu", rect(130, 20, 139, 30)),
                   ("In1.Cu", rect(141, 20, 150, 30))], 3)
    b.via(135 * MM, 25 * MM, 3)                              # F.Cu island and the first In1 island
    b.seg(145 * MM, 25 * MM, 155 * MM, 25 * MM, "In1.Cu", 3)  # second In1 island only
    zk = rect(165, 20, 170, 25)
    b.zone_ex(zk, [("In2.Cu", zk)], 0)                       # net-less zone with a net-less via
    b.via(167 * MM, 22 * MM, 0)
    # --- a teardrop zone (outline tests instead of triangles)
    td = [(170 * MM, 10 * MM - 100000), (173 * MM, 10 * MM - 500000), (173 * MM, 10 * MM + 500000),
          (170 * MM, 10 * MM + 100000)]
    b.zone_ex(td, [("F.Cu", td)], 1, teardrop=True)
    b.seg(168 * MM, y, 171 * MM, y, "F.Cu", 1)               # end anchor inside the outline
    b.via(173 * MM + 200000, y, 1)                           # ring overlaps the outline
    b.seg(171 * MM, 10 * MM + 400000, 175 * MM, 12 * MM, "F.Cu", 1)   # touches the slanted edge
    zr = rect(172, 8, 176, 12)
    b.zone_ex(zr, [("F.Cu", zr)], 1)                         # ordinary zone over the teardrop
    return b


def rev_ratsnest_zones():
    """REV-CN: RN_NET::OptimizeRNEdges (both lambdas) and ratsnest nodes next to zones."""
    b = Board(32, 4)
    # --- optimizeZoneAnchor: a lone via and a via in a zone island of the same net
    z = rect(30, 5, 50, 15)
    b.zone_ex(z, [("F.Cu", z)], 1)
    b.via(10 * MM, 10 * MM, 1)                               # two outline corners at one distance
    b.via(40 * MM, 10 * MM, 1)
    b.via(60 * MM, 4 * MM, 1, KEEP_ALL, "In1.Cu", "In2.Cu", kind="buried")  # not on the zone's layer
    b.seg(70 * MM, 12 * MM, 75 * MM, 12 * MM, "F.Cu", 1)     # a lone track
    # --- optimizeZoneToZoneAnchors: two islands of one zone, a via in each
    zz = rect(10, 30, 50, 40)
    b.zone_ex(zz, [("F.Cu", rect(10, 30, 20, 40)),
                   ("F.Cu", [(40 * MM, 31 * MM), (50 * MM, 30 * MM), (50 * MM, 40 * MM),
                             (38 * MM, 40 * MM)])], 2)
    b.via(12 * MM, 35 * MM, 2)
    b.via(48 * MM, 35 * MM, 2)
    # --- three islands, more than three candidate pairs, tracks ending in the fills
    zy = rect(10, 50, 90, 62)
    b.zone_ex(zy, [("B.Cu", rect(10, 50, 20, 60)), ("B.Cu", rect(30, 52, 40, 62)),
                   ("B.Cu", rect(60, 50, 70, 60)), ("In1.Cu", rect(10, 50, 22, 60)),
                   ("In1.Cu", rect(58, 50, 70, 60))], 3)
    b.via(15 * MM, 55 * MM, 3)                               # B.Cu and In1 islands at the left
    b.via(65 * MM, 55 * MM, 3)                               # B.Cu and In1 islands at the right
    b.seg(35 * MM, 57 * MM, 35 * MM, 70 * MM, "B.Cu", 3)     # middle island and a free end
    b.via(35 * MM, 70 * MM, 3)
    # --- a zone island without any item, and a net with zones only
    zq = rect(100, 5, 110, 15)
    b.zone_ex(zq, [("F.Cu", rect(100, 5, 104, 15)), ("F.Cu", rect(106, 5, 110, 15))], 4)
    return b


def rev_shapes():
    """REV-CN: copper graphics as connectivity items (connection points per shape kind) and the
    net propagation of clusters without pads (the first member with a net in list order)."""
    b = Board(33, 4)
    y = 10 * MM
    # the track's net wins over a net-less track and a net-less line (tracks come first)
    b.gr_line(10 * MM, y, 20 * MM, y, "F.Cu", 0)
    b.seg(20 * MM, y, 25 * MM, y, "F.Cu", 1)
    b.seg(10 * MM, y, 10 * MM, 5 * MM, "F.Cu", 0)
    # the line is the only item with a net
    b.gr_line(30 * MM, y, 40 * MM, y, "F.Cu", 2)
    b.seg(40 * MM, y, 45 * MM, y, "F.Cu", 0)
    b.seg(30 * MM, y, 30 * MM, 5 * MM, "F.Cu", 0)
    # two track nets in one cluster: the first in board order wins
    b.seg(50 * MM, y, 55 * MM, y, "F.Cu", 3)
    b.seg(55 * MM, y, 60 * MM, y, "F.Cu", 1)
    b.via(60 * MM, y, 2)
    b.seg(60 * MM, y, 65 * MM, y, "B.Cu", 4)
    # a free via keeps its net and does not stop the others
    b.seg(70 * MM, y, 75 * MM, y, "F.Cu", 0)
    b.via(75 * MM, y, 4, free=True)
    b.seg(75 * MM, y, 80 * MM, y, "B.Cu", 2)
    # circle (four key points), with a track on one and a zone under another
    b.gr_circle(100 * MM, y, 3 * MM, "F.Cu", 1)
    b.seg(103 * MM, y, 108 * MM, y, "F.Cu", 1)
    zc = rect(96, 8, 98, 12)
    b.zone_ex(zc, [("F.Cu", zc)], 1)
    b.seg(100 * MM, 12 * MM, 100 * MM, 9 * MM, "F.Cu", 1)    # inside the circle: no contact
    # rectangle outline (corners), filled rectangle (centre) in a zone, arc, filled polygon
    b.gr_rect(120 * MM, 5 * MM, 130 * MM, 15 * MM, "F.Cu", 1)
    b.seg(130 * MM, 15 * MM, 135 * MM, 18 * MM, "F.Cu", 1)
    b.gr_rect(140 * MM, 5 * MM, 150 * MM, 15 * MM, "B.Cu", 2, fill="yes")
    zr = rect(144, 9, 146, 11)
    b.zone_ex(zr, [("B.Cu", zr)], 2)
    zo = rect(151, 5, 155, 15)
    b.zone_ex(zo, [("B.Cu", zo)], 2)                         # 1 mm beside the filled rectangle
    b.gr_arc(160 * MM, y, 162 * MM, 12 * MM, 164 * MM, y, "In1.Cu", 3)
    b.seg(162 * MM, 12 * MM, 162 * MM, 16 * MM, "In1.Cu", 3)
    b.gr_poly([(170 * MM, 5 * MM), (180 * MM, 5 * MM), (175 * MM, 15 * MM)], "In2.Cu", 4)
    b.via(175 * MM, 8 * MM, 4)
    b.gr_poly([(185 * MM, 5 * MM), (195 * MM, 5 * MM), (190 * MM, 15 * MM)], "In2.Cu", 4, fill="no")
    zp = rect(189, 14, 191, 16)
    b.zone_ex(zp, [("In2.Cu", zp)], 4)
    # not copper: no item
    b.gr_line(10 * MM, 30 * MM, 20 * MM, 30 * MM, "Edge.Cuts", None)
    # two lines of different nets touching: the first drawing wins (no track in the cluster)
    b.gr_line(30 * MM, 30 * MM, 40 * MM, 30 * MM, "B.Cu", 3)
    b.gr_line(40 * MM, 30 * MM, 50 * MM, 30 * MM, "B.Cu", 1)
    # SHAPE_SEGMENT / SHAPE_SEGMENT collision rounds the two half widths differently
    # ((wA + 1) / 2 + wB / 2): parallel tracks 200000 nm apart with an odd and an even width
    # touch only with the odd one as the first operand. C++ tests a pair from both sides.
    for k, (w1, w2) in enumerate([("0.200001", "0.2"), ("0.2", "0.200001"), ("0.200001", "0.200001"),
                                  ("0.2", "0.2")]):
        yy = (40 + 2 * k) * MM
        b.seg(10 * MM, yy, 20 * MM, yy, "In1.Cu", 1, width=w1)
        b.seg(10 * MM, yy + 200000, 20 * MM, yy + 200000, "In1.Cu", 1, width=w2)
    return b


def rev_order_probe():
    """REV-CN: results that follow the order of CN_ITEM::m_connected, which the reference build
    keeps in heap-address order (rust/docs/design/review-connectivity.md)."""
    b = Board(34, 3)
    # a track whose two ends lie off-centre in two vias (DRC: track not centred on via, first hit)
    b.via(10 * MM, 10 * MM, 1)
    b.via(20 * MM, 10 * MM, 1)
    b.seg(10 * MM + 50000, 10 * MM, 20 * MM - 50000, 10 * MM, "F.Cu", 1)
    # optimizeZoneAnchor: the nearest outline vertex is the same point on two zones
    z = rect(30, 25, 50, 35)
    b.zone_ex(z, [("F.Cu", z)], 2)
    b.zone_ex(z, [("B.Cu", z)], 2)
    b.via(10 * MM, 28 * MM, 2)
    b.via(40 * MM, 30 * MM, 2)
    # optimizeZoneToZoneAnchors: four candidate pairs with one centre distance, different outlines
    zp = rect(10, 50, 50, 60)
    b.zone_ex(zp, [("F.Cu", rect(10, 50, 20, 60)), ("F.Cu", rect(40, 50, 50, 60))], 3)
    qa = [(10 * MM, 50 * MM), (18 * MM, 50 * MM), (20 * MM, 55 * MM), (18 * MM, 60 * MM),
          (10 * MM, 60 * MM)]
    qb = [(40 * MM, 57 * MM), (42 * MM, 50 * MM), (50 * MM, 50 * MM), (50 * MM, 60 * MM),
          (42 * MM, 60 * MM)]
    b.zone_ex(zp, [("B.Cu", qa), ("B.Cu", qb)], 3)
    b.via(12 * MM, 55 * MM, 3)
    b.via(48 * MM, 55 * MM, 3)
    return b


BOARDS = {
    "cn04_flash": flash,
    "cn04_dangling": dangling,
    "cn04_random1": lambda: random_board(21),
    "cn04_random2": lambda: random_board(22),
    "rev_zone_contacts": rev_zone_contacts,
    "rev_ratsnest_zones": rev_ratsnest_zones,
    "rev_shapes": rev_shapes,
    "rev_order_probe": rev_order_probe,
}

if __name__ == "__main__":
    out = sys.argv[1]
    for name, make in BOARDS.items():
        with open(os.path.join(out, name + ".kicad_pcb"), "w") as f:
            f.write(make().text())
