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
"""Crafted board: graphic shapes with every stroke style / fill / degenerate geometry on copper, silk, mask, edge."""
import sys, math
from common import *
out = header(LAYERS2)
S = []
def stroke(w, t): return "(stroke (width %g) (type %s))" % (w, t)
def add(s): S.append(" " + s + "\n")
styles = ["solid", "default", "dash", "dot", "dash_dot", "dash_dot_dot"]
y = 10
for li, layer in enumerate(["F.Cu", "B.Cu", "F.SilkS", "Edge.Cuts", "F.Mask", "Dwgs.User"]):
    x0 = 10 + li * 0  # same x, different y band per layer
    for si, st in enumerate(styles):
        for wi, w in enumerate((0.1, 0.5, 0)):
            x = 10 + si * 30 + wi * 9; yy = y
            n = ' (net 1)' if layer.endswith(".Cu") else ''
            add('(gr_line (start %g %g) (end %g %g) %s (layer "%s")%s (uuid "%s"))' % (x, yy, x + 7, yy + 3, stroke(w, st), layer, n, uid()))
            add('(gr_rect (start %g %g) (end %g %g) %s (fill none) (layer "%s")%s (uuid "%s"))' % (x, yy + 4, x + 7, yy + 8, stroke(w, st), layer, n, uid()))
            add('(gr_circle (center %g %g) (end %g %g) %s (fill none) (layer "%s")%s (uuid "%s"))' % (x + 3.5, yy + 12, x + 6, yy + 12, stroke(w, st), layer, n, uid()))
            add('(gr_arc (start %g %g) (mid %g %g) (end %g %g) %s (layer "%s")%s (uuid "%s"))' % (x, yy + 18, x + 3.5, yy + 15.5, x + 7, yy + 18, stroke(w, st), layer, n, uid()))
            add('(gr_poly (pts (xy %g %g) (xy %g %g) (xy %g %g) (xy %g %g)) %s (fill none) (layer "%s")%s (uuid "%s"))' % (x, yy + 20, x + 7, yy + 20, x + 5, yy + 24, x + 1, yy + 23, stroke(w, st), layer, n, uid()))
            add('(gr_curve (pts (xy %g %g) (xy %g %g) (xy %g %g) (xy %g %g)) %s (layer "%s")%s (uuid "%s"))' % (x, yy + 26, x + 2, yy + 22, x + 5, yy + 30, x + 7, yy + 26, stroke(w, st), layer, n, uid()))
    y += 34
# fills (solid, hatch kinds) on copper and silk
y0 = y
for li, layer in enumerate(["F.Cu", "F.SilkS", "B.SilkS"]):
    for fi, fill in enumerate(["solid", "hatch", "reverse_hatch", "cross_hatch", "yes"]):
        for wi, w in enumerate((0.15, 0)):
            x = 10 + fi * 36 + wi * 17; yy = y0 + li * 30
            add('(gr_rect (start %g %g) (end %g %g) %s (fill %s) (layer "%s") (uuid "%s"))' % (x, yy, x + 14, yy + 6, stroke(w, "solid"), fill, layer, uid()))
            add('(gr_circle (center %g %g) (end %g %g) %s (fill %s) (layer "%s") (uuid "%s"))' % (x + 4, yy + 11, x + 7.5, yy + 11, stroke(w, "dash" if fi == 1 else "solid"), fill, layer, uid()))
            add('(gr_poly (pts (xy %g %g) (xy %g %g) (xy %g %g) (xy %g %g) (xy %g %g)) %s (fill %s) (layer "%s") (uuid "%s"))' % (x, yy + 16, x + 14, yy + 17, x + 12, yy + 26, x + 7, yy + 21, x + 2, yy + 27, stroke(w, "solid"), fill, layer, uid()))
y = y0 + 95
# arcs near 0 / 360 degrees, tiny and huge, as graphic arcs and as tracks
def arc_pts(cx, cy, r, a0, sweep):
    a1 = a0 + sweep; am = a0 + sweep / 2
    p = lambda a: (cx + r * math.cos(math.radians(a)), cy + r * math.sin(math.radians(a)))
    return p(a0), p(am), p(a1)
k = 0
for sweep in (0.02, 0.5, 5, 89.999, 90, 179.99, 180, 180.01, 270, 359, 359.9, 359.99, -0.5, -90, -180, -270, -359.9):
    for r in (0.05, 0.5, 3, 40):
        if r == 40 and abs(sweep) > 20: continue
        cx = 12 + (k % 14) * 14; cy = y + (k // 14) * 9; k += 1
        (sx, sy), (mx, my), (ex, ey) = arc_pts(cx, cy, r, 33, sweep)
        for layer in ("F.Cu", "F.SilkS", "Edge.Cuts"):
            add('(gr_arc (start %.6f %.6f) (mid %.6f %.6f) (end %.6f %.6f) %s (layer "%s") (uuid "%s"))' % (sx, sy, mx, my, ex, ey, stroke(0.15, "solid"), layer, uid()))
        (sx, sy), (mx, my), (ex, ey) = arc_pts(cx, cy + 4, r, 200, sweep)
        add('(arc (start %.6f %.6f) (mid %.6f %.6f) (end %.6f %.6f) (width 0.2) (layer "B.Cu") (net 2) (uuid "%s"))' % (sx, sy, mx, my, ex, ey, uid()))
y += (k // 14 + 1) * 9 + 4
# degenerate shapes
x = 10
for layer in ("F.Cu", "F.SilkS", "Edge.Cuts", "F.Mask"):
    add('(gr_line (start %g %g) (end %g %g) %s (layer "%s") (uuid "%s"))' % (x, y, x, y, stroke(0.2, "solid"), layer, uid()))
    add('(gr_line (start %g %g) (end %g %g) %s (layer "%s") (uuid "%s"))' % (x + 3, y, x + 3, y, stroke(0.2, "dash"), layer, uid()))
    add('(gr_circle (center %g %g) (end %g %g) %s (fill none) (layer "%s") (uuid "%s"))' % (x + 6, y, x + 6, y, stroke(0.2, "solid"), layer, uid()))
    add('(gr_circle (center %g %g) (end %g %g) %s (fill solid) (layer "%s") (uuid "%s"))' % (x + 9, y, x + 9, y, stroke(0, "solid"), layer, uid()))
    add('(gr_rect (start %g %g) (end %g %g) %s (fill none) (layer "%s") (uuid "%s"))' % (x + 12, y, x + 12, y, stroke(0.2, "solid"), layer, uid()))
    add('(gr_rect (start %g %g) (end %g %g) %s (fill solid) (layer "%s") (uuid "%s"))' % (x + 15, y, x + 18, y, stroke(0, "solid"), layer, uid()))
    add('(gr_poly (pts (xy %g %g) (xy %g %g)) %s (fill solid) (layer "%s") (uuid "%s"))' % (x + 21, y, x + 24, y + 1, stroke(0.1, "solid"), layer, uid()))
    add('(gr_poly (pts (xy %g %g) (xy %g %g) (xy %g %g)) %s (fill solid) (layer "%s") (uuid "%s"))' % (x + 27, y, x + 30, y, x + 33, y, stroke(0, "solid"), layer, uid()))
    add('(gr_curve (pts (xy %g %g) (xy %g %g) (xy %g %g) (xy %g %g)) %s (layer "%s") (uuid "%s"))' % (x + 36, y, x + 36, y, x + 36, y, x + 36, y, stroke(0.1, "solid"), layer, uid()))
    add('(gr_line (start %g %g) (end %g %g) %s (layer "%s") (uuid "%s"))' % (x + 40, y, x + 40.000002, y, stroke(0.3, "dash_dot"), layer, uid()))
    add('(gr_rect (start %g %g) (end %g %g) %s (fill solid) (layer "%s") (uuid "%s"))' % (x + 50, y + 1, x + 44, y - 1, stroke(0.1, "dot"), layer, uid()))
    y += 5
# copper shapes with solder mask + tracks of several kinds
add('(gr_rect (start 100 %g) (end 110 %g) %s (fill solid) (layer "F.Cu") (solder_mask_margin 0.1) (net 1) (uuid "%s"))' % (y, y + 5, stroke(0.1, "solid"), uid()))
add('(gr_circle (center 120 %g) (end 123 %g) %s (fill none) (layers "F.Cu" "F.Mask") (solder_mask_margin 0.2) (net 2) (uuid "%s"))' % (y + 3, y + 3, stroke(0.3, "solid"), uid()))
add('(gr_poly (pts (xy 130 %g) (xy 140 %g) (xy 135 %g)) %s (fill solid) (layers "B.Cu" "B.Mask") (solder_mask_margin -0.05) (net 1) (uuid "%s"))' % (y, y, y + 6, stroke(0.2, "solid"), uid()))
add('(gr_line (start 145 %g) (end 160 %g) %s (layers "F.Cu" "F.Mask") (net 1) (uuid "%s"))' % (y, y + 5, stroke(0.4, "dash"), uid()))
add('(segment (start 10 %g) (end 30 %g) (width 0.25) (layer "F.Cu") (net 1) (uuid "%s"))' % (y + 10, y + 10, uid()))
add('(segment (start 30 %g) (end 30 %g) (width 0.25) (layer "F.Cu") (net 1) (uuid "%s"))' % (y + 10, y + 10, uid()))
add('(segment (start 40 %g) (end 60 %g) (width 0) (layer "F.Cu") (net 1) (uuid "%s"))' % (y + 10, y + 12, uid()))
add('(segment (start 70 %g) (end 90 %g) (width 1.5) (layers "F.Cu" "F.Mask") (solder_mask_margin 0.1) (net 2) (uuid "%s"))' % (y + 10, y + 12, uid()))
add('(segment (start 70 %g) (end 90 %g) (width 0.5) (locked yes) (layer "B.Cu") (net 0) (uuid "%s"))' % (y + 14, y + 14, uid()))
out += "".join(S) + edge(0, 0, 215, y + 25) + ")\n"
sys.stdout.write(out)
