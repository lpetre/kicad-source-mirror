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
"""Crafted board: zones (solid / hatch / islands / multi-layer / keepout / non-copper), thermal custom pads, vias, NPTH."""
import sys
from common import *
out = header(LAYERS4)
S = []
def add(s): S.append(" " + s + "\n")
def poly(pts): return "(polygon (pts %s))" % " ".join("(xy %g %g)" % p for p in pts)
def rect(x0, y0, x1, y1): return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
def zone(netn, layers, pts, fill, extra="", prio=0, name=""):
    lay = '(layer "%s")' % layers[0] if len(layers) == 1 else "(layers %s)" % " ".join('"%s"' % l for l in layers)
    return '(zone (net %d) (net_name "%s") %s (uuid "%s") %s (hatch edge 0.5) %s (connect_pads (clearance 0.3)) (min_thickness 0.25) %s %s %s)' % (
        NETS[netn], netn, lay, uid(), '(name "%s")' % name if name else "", "(priority %d)" % prio if prio else "", extra, fill, poly(pts))
solid = "(fill yes (thermal_gap 0.4) (thermal_bridge_width 0.5) (island_removal_mode 1) (island_area_min 10))"
hatch = "(fill yes (mode hatch) (thermal_gap 0.5) (thermal_bridge_width 0.5) (smoothing fillet) (radius 1) (island_removal_mode 0) (hatch_thickness 0.6) (hatch_gap 1.2) (hatch_orientation 30) (hatch_smoothing_level 1) (hatch_smoothing_value 0.1) (hatch_border_algorithm hatch_thickness) (hatch_min_hole_area 0.3))"
# footprint with pads inside the zones (thermal reliefs incl. custom pads)
smd = '(layers "F.Cu" "F.Mask" "F.Paste")'; tht = '(layers "*.Cu" "*.Mask")'
prim = '(primitives (gr_poly (pts (xy -1.2 -0.8) (xy 1.4 -1) (xy 1 0.9) (xy -1.1 0.7)) (width 0.1) (fill yes)) (gr_circle (center 1.5 1.2) (end 1.9 1.2) (width 0.1) (fill yes)))'
def fp(ref, x, y, rot, pads):
    s = ' (footprint "rev:z_%s" (layer "F.Cu") (uuid "%s") (at %g %g %g)\n' % (ref, uid(), x, y, rot)
    s += '  (property "Reference" "%s" (at 0 -4 %g) (layer "F.SilkS") (uuid "%s") (effects (font (size 1 1) (thickness 0.15))))\n' % (ref, rot, uid())
    s += '  (attr smd)\n' + "".join("  " + p + "\n" for p in pads) + " )\n"
    S.append(s)
def pad(num, typ, shape, x, y, a, body, netn):
    return '(pad "%s" %s %s (at %g %g %g) %s %s (uuid "%s"))' % (num, typ, shape, x, y, a, body, net(netn), uid())
for i, (rot, x, y) in enumerate(((0, 30, 30), (37, 75, 30), (90, 30, 75), (0, 75, 75))):
    fp("Z%d" % i, x, y, rot, [
        pad(1, "smd", "custom", -8, 0, rot + 20, "(size 1 1) %s (zone_connect 1) (thermal_bridge_width 0.3) (thermal_gap 0.35) (thermal_bridge_angle 30) (options (clearance outline) (anchor circle)) %s" % (smd, prim), "GND"),
        pad(2, "smd", "custom", -2, 0, rot, "(size 1 1) %s (options (clearance convexhull) (anchor rect)) %s" % (smd, prim), "GND"),
        pad(3, "thru_hole", "circle", 4, 0, rot, "(size 1.8 1.8) (drill 0.9) %s (zone_connect 1) (thermal_bridge_angle 45)" % tht, "GND"),
        pad(4, "thru_hole", "oval", 9, 0, rot + 30, "(size 2.4 1.4) (drill oval 1.4 0.7) %s (zone_connect 2)" % tht, "GND"),
        pad(5, "smd", "roundrect", -8, 5, rot, "(size 2 1.2) %s (roundrect_rratio 0.25) (zone_connect 0)" % smd, "GND"),
        pad(6, "smd", "rect", -2, 5, rot + 45, "(size 2 1.2) %s (zone_connect 3)" % smd, "GND"),
        pad(7, "thru_hole", "rect", 4, 5, rot, "(size 1.8 1.8) (drill 0.9) %s" % tht, "N1"),
        pad("", "np_thru_hole", "circle", 9, 5, rot, "(size 2.2 2.2) (drill 1.1) %s" % tht, ""),
        pad("", "np_thru_hole", "circle", 9, -5, rot, "(size 1.1 1.1) (drill 1.1) %s" % tht, ""),
        pad(8, "thru_hole", "oval", 4, -5, rot, "(size 2 1.2) (property pad_prop_castellated) (drill 0.7) %s" % tht, "GND"),
    ])
add(zone("GND", ["F.Cu"], rect(10, 10, 100, 50), solid, name="solid top"))
add(zone("GND", ["F.Cu", "B.Cu", "In1.Cu"], rect(10, 55, 100, 95), hatch, name="hatched multi"))
add(zone("N1", ["In2.Cu"], rect(10, 10, 100, 95), solid))
add(zone("N2", ["B.Cu"], rect(10, 10, 100, 50), solid.replace("(island_removal_mode 1)", "(island_removal_mode 2)"), "(filled_areas_thickness yes)", prio=1))
add(zone("", ["F.Cu", "B.Cu"], rect(40, 38, 60, 46), "(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))", "(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))"))
add(zone("", ["F.SilkS"], rect(110, 10, 140, 30), solid))
add(zone("", ["F.Mask"], rect(110, 35, 140, 50), solid))
add(zone("", ["Dwgs.User"], [(110, 55), (140, 55), (125, 80)], hatch))
add(zone("GND", ["F.Cu"], [(110, 60), (150, 60), (150, 95), (130, 70), (110, 95)], solid, "(filled_areas_thickness yes)"))
# tracks cutting zones into islands, vias of every kind
add('(segment (start 10 22) (end 100 22) (width 0.4) (layer "F.Cu") (net 1) (uuid "%s"))' % uid())
add('(segment (start 55 10) (end 55 50) (width 0.4) (layer "F.Cu") (net 2) (uuid "%s"))' % uid())
add('(segment (start 10 60) (end 100 90) (width 0.6) (layer "B.Cu") (net 1) (uuid "%s"))' % uid())
add('(arc (start 20 12) (mid 30 16) (end 40 12) (width 0.3) (layer "F.Cu") (net 2) (uuid "%s"))' % uid())
k = 0
for typ, layers in (("", ("F.Cu", "B.Cu")), ("blind ", ("F.Cu", "In1.Cu")), ("blind ", ("In2.Cu", "B.Cu")), ("micro ", ("F.Cu", "In1.Cu")), ("blind ", ("In1.Cu", "In2.Cu"))):
    for netn in ("GND", "N1", ""):
        for extra in ("", "(remove_unused_layers yes) (keep_end_layers yes)", "(remove_unused_layers yes) (keep_end_layers no)", "(free yes)", "(locked yes)"):
            x = 12 + (k % 15) * 5.7; y = 14 + (k // 15) * 16; k += 1
            add('(via %s(at %g %g) (size %g) (drill %g) (layers "%s" "%s") %s (net %d) (uuid "%s"))' % (typ, x, y, 0.9 if k % 2 else 0.6, 0.4 if k % 2 else 0.3, layers[0], layers[1], extra, NETS[netn], uid()))
out += "".join(S) + edge(0, 0, 160, 105) + ")\n"
sys.stdout.write(out)
