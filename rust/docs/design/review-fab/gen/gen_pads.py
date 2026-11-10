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
"""Crafted board: every pad kind in rotated / flipped footprints (KiCad 8 syntax; upgraded by the reference)."""
import sys, math
from common import *
def pads(side, fprot):
    F, B = ("F", "B") if side == "F" else ("B", "F")
    smd = '(layers "%s.Cu" "%s.Mask" "%s.Paste")' % (F, F, F)
    tht = '(layers "*.Cu" "*.Mask")'
    out = []; k = [0]
    def pos():
        i = k[0]; k[0] += 1
        x = (i % 8) * 4.0 - 14.0; y = (i // 8) * 4.0 - 10.0
        return x, y
    def pad(num, typ, shape, body, rot=0, netn="N1"):
        x, y = pos()
        a = fprot + rot
        at = "(at %g %g %g)" % (x, y, a) if a else "(at %g %g)" % (x, y)
        out.append('  (pad "%s" %s %s %s %s %s (uuid "%s"))' % (num, typ, shape, at, body, net(netn), uid()))
    n = 1
    # plain shapes, each also rotated
    for rot in (0, 20, 90, 133):
        pad(n, "smd", "rect", "(size 1.5 1) " + smd, rot); n += 1
        pad(n, "smd", "oval", "(size 2 1) " + smd, rot, "N2"); n += 1
        pad(n, "smd", "circle", "(size 1.2 1.2) " + smd, rot); n += 1
        pad(n, "smd", "roundrect", "(size 2 1) %s (roundrect_rratio 0.25)" % smd, rot, "GND"); n += 1
        pad(n, "smd", "roundrect", "(size 1.6 1.6) %s (roundrect_rratio 0.5)" % smd, rot); n += 1
        pad(n, "smd", "roundrect", "(size 2 1.2) %s (roundrect_rratio 0)" % smd, rot); n += 1
        pad(n, "smd", "trapezoid", "(size 2 1.5) (rect_delta 0.4 0) " + smd, rot); n += 1
        pad(n, "smd", "trapezoid", "(size 2 1.5) (rect_delta 0 -0.6) " + smd, rot, "N2"); n += 1
    # chamfered
    for rot, ch, rr in ((0, "top_left", 0), (15, "top_left bottom_right", 0), (90, "top_left top_right bottom_left bottom_right", 0),
                        (0, "top_right", 0.2), (33, "bottom_left bottom_right", 0.1), (270, "top_left top_right bottom_left", 0.25),
                        (0, "top_left top_right bottom_left bottom_right", 0.25), (45, "bottom_right", 0)):
        pad(n, "smd", "roundrect", "(size 2.2 1.4) %s (roundrect_rratio %g) (chamfer_ratio 0.2) (chamfer %s)" % (smd, rr, ch), rot); n += 1
    # offsets (shape offset from hole / pad position)
    pad(n, "smd", "rect", "(size 1.5 1) (drill (offset 0.4 0.2)) " + smd, 0); n += 1
    pad(n, "smd", "oval", "(size 2 1) (drill (offset -0.3 0.5)) " + smd, 30); n += 1
    pad(n, "smd", "roundrect", "(size 2 1) (drill (offset 0.5 0)) %s (roundrect_rratio 0.3)" % smd, 100); n += 1
    pad(n, "thru_hole", "oval", "(size 2.4 1.4) (drill 0.8 (offset 0.5 0)) " + tht, 0, "N2"); n += 1
    pad(n, "thru_hole", "rect", "(size 2 1.6) (drill oval 1.2 0.6 (offset 0 0.3)) " + tht, 45, "N2"); n += 1
    pad(n, "thru_hole", "circle", "(size 1.6 1.6) (drill 0.8) " + tht); n += 1
    pad(n, "thru_hole", "roundrect", "(size 1.8 1.8) (drill 0.9) %s (roundrect_rratio 0.2)" % tht, 22.5); n += 1
    pad(n, "thru_hole", "oval", "(size 1.4 2.6) (drill oval 0.7 1.9) " + tht, 60); n += 1
    pad(n, "thru_hole", "trapezoid", "(size 2 2) (rect_delta 0.5 0) (drill 0.8) " + tht, 10); n += 1
    pad(n, "thru_hole", "circle", "(size 1.6 1.6) (drill 0.8) %s (remove_unused_layers yes) (keep_end_layers yes)" % tht); n += 1
    pad(n, "thru_hole", "circle", "(size 1.6 1.6) (drill 0.8) %s (remove_unused_layers yes) (keep_end_layers no)" % tht, 0, "GND"); n += 1
    pad("", "np_thru_hole", "circle", "(size 1 1) (drill 1) " + tht, 0, "")
    pad("", "np_thru_hole", "circle", "(size 2 2) (drill 1) " + tht, 0, "")
    pad("", "np_thru_hole", "oval", "(size 2 1) (drill oval 2 1) " + tht, 40, "")
    pad("", "np_thru_hole", "oval", "(size 3 2) (drill oval 2 1) " + tht, 120, "")
    pad("", "np_thru_hole", "rect", "(size 2.5 2.5) (drill 1.2) " + tht, 15, "")
    # fabrication properties
    for prop in ("pad_prop_bga", "pad_prop_fiducial_glob", "pad_prop_fiducial_loc", "pad_prop_testpoint", "pad_prop_heatsink", "pad_prop_castellated"):
        if prop == "pad_prop_castellated":
            pad(n, "thru_hole", "oval", "(size 2 1.2) (property %s) (drill 0.7) %s" % (prop, tht), 0); n += 1
        else:
            pad(n, "smd", "circle" if "fid" in prop or "bga" in prop else "rect", "(size 1.2 1.2) (property %s) %s" % (prop, smd), 12); n += 1
    pad(n, "thru_hole", "circle", "(size 1.6 1.6) (property pad_prop_heatsink) (drill 0.8) " + tht); n += 1
    # margins / pin function
    pad(n, "smd", "rect", "(size 1.5 1) %s (solder_mask_margin 0.2)" % smd, 20); n += 1
    pad(n, "smd", "rect", "(size 1.5 1) %s (solder_mask_margin -0.1)" % smd, 0); n += 1
    pad(n, "smd", "oval", "(size 2 1) %s (solder_mask_margin 0.15) (solder_paste_margin -0.1)" % smd, 70); n += 1
    pad(n, "smd", "roundrect", "(size 2 1) %s (roundrect_rratio 0.25) (solder_mask_margin 0.1) (solder_paste_margin_ratio -0.2)" % smd, 33); n += 1
    pad(n, "smd", "trapezoid", "(size 2 1.5) (rect_delta 0.4 0) %s (solder_mask_margin 0.1) (solder_paste_margin -0.2)" % smd, 50); n += 1
    pad(n, "smd", "circle", "(size 1.2 1.2) %s (solder_paste_margin 0.1) (solder_paste_margin_ratio 0.1)" % smd); n += 1
    pad(n, "smd", "roundrect", "(size 2.2 1.4) %s (roundrect_rratio 0.1) (chamfer_ratio 0.25) (chamfer top_left bottom_right) (solder_mask_margin 0.12) (solder_paste_margin -0.07)" % smd, 77); n += 1
    pad(n, "smd", "rect", '(size 1.5 1) %s (pinfunction "CLK,1 *%%") (pintype "input")' % smd, 0); n += 1
    pad(n, "connect", "rect", '(size 1.5 1) (layers "%s.Cu" "%s.Mask")' % (F, F), 5); n += 1
    pad(n, "smd", "rect", '(size 1.5 1) (layers "%s.Cu")' % F, 5); n += 1
    pad("", "smd", "rect", '(size 1.5 1) (layers "%s.Paste")' % F, 25, "")
    pad("", "smd", "roundrect", '(size 1.5 1) (layers "%s.Mask") (roundrect_rratio 0.25)' % F, 25, "")
    pad(n, "smd", "oval", '(size 2 1) (layers "%s.Cu" "%s.Mask" "%s.Paste")' % (B, B, B), 25); n += 1
    pad(n, "thru_hole", "oval", '(size 2 1.4) (drill 0.8) (layers "*.Cu" "*.Mask" "*.Paste")', 25); n += 1
    pad(n, "thru_hole", "rect", '(size 1.7 1.7) (drill 1) (layers "F.Cu" "B.Cu")', 0); n += 1
    # custom pads
    prim = '(primitives (gr_poly (pts (xy -1 -0.6) (xy 1.2 -0.9) (xy 0.8 0.7) (xy -0.9 0.5)) (width 0.1) (fill yes)) (gr_line (start 0 0) (end 1.6 1.1) (width 0.25)) (gr_arc (start -1 0.8) (mid -1.5 0) (end -1 -0.8) (width 0.15)) (gr_circle (center 0.3 -1.2) (end 0.6 -1.2) (width 0.1) (fill yes)) (gr_circle (center -0.6 1.2) (end -0.3 1.2) (width 0.12)) (gr_rect (start 1 -0.3) (end 1.8 0.3) (width 0) (fill yes)))'
    pad(n, "smd", "custom", "(size 0.8 0.8) %s (options (clearance outline) (anchor rect)) %s" % (smd, prim), 0); n += 1
    pad(n, "smd", "custom", "(size 0.8 0.8) %s (options (clearance convexhull) (anchor circle)) %s" % (smd, prim), 25, "N2"); n += 1
    pad(n, "smd", "custom", "(size 0.8 0.8) %s (zone_connect 2) (thermal_bridge_width 0.3) (thermal_gap 0.3) (thermal_bridge_angle 45) (options (clearance outline) (anchor rect)) %s (solder_mask_margin 0.1) (solder_paste_margin -0.05)" % (smd, prim), 190, "GND"); n += 1
    pad(n, "thru_hole", "custom", "(size 1.4 1.4) (drill 0.7) %s (options (clearance outline) (anchor circle)) (primitives (gr_poly (pts (xy -1.5 -1) (xy 1.5 -1) (xy 1.5 1) (xy 0 1.6) (xy -1.5 1)) (width 0) (fill yes)))" % tht, 300, "GND"); n += 1
    pad(n, "smd", "custom", "(size 1 1) (drill (offset 0.3 0.3)) %s (options (clearance outline) (anchor rect)) (primitives (gr_poly (pts (xy -1 -1) (xy 1 -1) (xy 1 1) (xy -1 1)) (width 0.2) (fill yes)))" % smd, 45, "GND"); n += 1
    return out
def footprint(ref, x, y, rot, side, extra=""):
    F = side
    s = ' (footprint "rev:pads_%s" (layer "%s.Cu") (uuid "%s") (at %g %g%s)\n' % (ref, F, uid(), x, y, (" %g" % rot) if rot else "")
    mir = " (justify mirror)" if F == "B" else ""
    s += '  (property "Reference" "%s" (at 0 -13 %g) (layer "%s.SilkS") (uuid "%s") (effects (font (size 1 1) (thickness 0.15))%s))\n' % (ref, rot, F, uid(), mir)
    s += '  (property "Value" "val_%s" (at 0 22 %g) (layer "%s.Fab") (uuid "%s") (effects (font (size 1 1) (thickness 0.15))%s))\n' % (ref, rot, F, uid(), mir)
    s += '  (attr smd%s)\n' % extra
    s += '  (fp_rect (start -17 -12) (end 17 21) (stroke (width 0.1) (type default)) (fill none) (layer "%s.CrtYd") (uuid "%s"))\n' % (F, uid())
    s += '  (fp_line (start -17 -12) (end 17 21) (stroke (width 0.12) (type dash)) (layer "%s.Fab") (uuid "%s"))\n' % (F, uid())
    s += "\n".join(pads(side, rot)) + "\n )\n"
    return s
out = header()
insts = [("U1", 30, 30, 0, "F"), ("U2", 80, 30, 33, "F"), ("U3", 135, 30, 90, "F"), ("U4", 185, 30, 180, "F"),
         ("U5", 30, 90, 270, "F"), ("U6", 80, 90, -45.5, "F"), ("U7", 135, 90, 0, "B"), ("U8", 185, 90, 33, "B"),
         ("U9", 30, 150, 90, "B"), ("U10", 80, 150, 210, "B"), ("U11", 135, 150, 0.7, "F"), ("U12", 185, 150, 179.9, "B")]
sel = sys.argv[1].split(",") if len(sys.argv) > 1 else None
for r, x, y, rot, side in insts:
    if sel and r not in sel: continue
    out += footprint(r, x, y, rot, side, " dnp" if r in ("U6", "U8") else "")
out += edge(0, 0, 215, 180) + ")\n"
sys.stdout.write(out)
