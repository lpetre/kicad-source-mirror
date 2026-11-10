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
"""Crafted board: texts, text boxes, tables, dimensions in many states (KiCad 8 syntax)."""
import sys
from common import *
out = header(LAYERS2)
S = []
def add(s): S.append(" " + s + "\n")
def eff(size=(1, 1), th=0.15, bold=False, italic=False, just="", extra=""):
    f = "(font (size %g %g) (thickness %g)%s%s)" % (size[0], size[1], th, " (bold yes)" if bold else "", " (italic yes)" if italic else "")
    j = " (justify %s)" % just if just else ""
    return "(effects %s%s%s)" % (f, j, extra)
texts = ["Plain", "Two\\nLines", "A\\nB\\nC d", "~{RST} x^{2} y_{i}", "Tab\\tbed  spaces", "éüßΩ µm", "${REFERENCE}_${LAYER}_${NOPE}", "W", "", " ", "\\\\slash \\\"q\\\""]
k = 0
for layer, mir in (("F.Cu", ""), ("F.SilkS", ""), ("B.SilkS", "mirror"), ("B.Cu", "mirror"), ("F.Mask", ""), ("Edge.Cuts", "")):
    for ti, t in enumerate(texts):
        for vi, (rot, just, ko, bold, ital, size, th) in enumerate((
                (0, "", False, False, False, (1, 1), 0.15), (33, "left", False, True, False, (1.5, 1), 0.3),
                (90, "right top", True, False, True, (1, 1.6), 0.12), (180, "left bottom", False, True, True, (2, 2), 0.4),
                (270, "top", True, False, False, (0.8, 0.8), 0.1), (-45.5, "bottom", False, False, False, (1, 1), 0.02))):
            x = 8 + (k % 22) * 9.3; y = 8 + (k // 22) * 9; k += 1
            j = (just + " " + mir).strip()
            lay = '(layer "%s"%s)' % (layer, " knockout" if ko else "")
            add('(gr_text "%s" (at %g %g %g) %s (uuid "%s") %s)' % (t, x, y, rot, lay, uid(), eff(size, th, bold, ital, j)))
y = 8 + (k // 22 + 1) * 9
# text boxes
k = 0
for layer in ("F.Cu", "F.SilkS", "B.SilkS"):
    for rot in (0, 90, 30, 180, 270, -60):
        for border, ko in ((True, False), (False, False), (True, False)):
            x = 10 + (k % 9) * 22; yy = y + (k // 9) * 14; k += 1
            mir = " mirror" if layer.startswith("B.") else ""
            st = "(border yes) (stroke (width 0.15) (type %s))" % ("dash" if rot == 90 else "solid") if border else "(border no) (stroke (width 0.1) (type solid))"
            if rot in (0, 90, 180, 270):
                geo = "(start %g %g) (end %g %g)" % (x, yy, x + 16, yy + 8)
            else:
                import math
                c, s_ = math.cos(math.radians(rot)), math.sin(math.radians(rot))
                pts = [(0, 0), (14, 0), (14, 7), (0, 7)]
                geo = "(pts %s)" % " ".join("(xy %.6f %.6f)" % (x + 4 + px * c - py * s_, yy + 4 + px * s_ + py * c) for px, py in pts)
            add('(gr_text_box "Box %d\\nline two is long" %s (margins 0.5 0.4 0.6 0.3) (angle %g) (layer "%s"%s) (uuid "%s") %s %s)' % (
                k, geo, rot, layer, " knockout" if ko else "", uid(), eff((1, 1), 0.15, k % 4 == 0, k % 5 == 0, ("left top" if k % 3 else "right bottom") + mir), st))
y = y + (k // 9 + 1) * 14 + 4
# dimensions
k = 0
def dim(typ, layer, pts, extra, fmt, style, txt_at, rot=0):
    mir = " (justify mirror)" if layer.startswith("B.") else ""
    if typ == 'center':
        return '(dimension (type center) (layer "%s") (uuid "%s") (pts %s) (style %s))' % (layer, uid(), " ".join("(xy %g %g)" % p for p in pts), style)
    return '(dimension (type %s) (layer "%s") (uuid "%s") (pts %s) %s (gr_text "X" (at %g %g %g) (layer "%s") (uuid "%s") (effects (font (size 1 1) (thickness 0.15))%s)) (format %s) (style %s))' % (
        typ, layer, uid(), " ".join("(xy %g %g)" % p for p in pts), extra, txt_at[0], txt_at[1], rot, layer, uid(), mir, fmt, style)
for layer in ("F.Cu", "F.SilkS", "B.SilkS", "Dwgs.User"):
    for vi in range(6):
        x = 10 + (k % 6) * 34; yy = y + (k // 6) * 20; k += 1
        units = vi % 4; ufmt = vi % 4; prec = (vi * 2) % 7; sz = "(suppress_zeroes yes)" if vi % 2 else ""
        ov = '(override_value "OVR %d")' % vi if vi == 3 else ""
        fmt = '(prefix "p%d ") (suffix " s") (units %d) (units_format %d) (precision %d) %s %s' % (vi, units, ufmt, prec, ov, sz)
        tpm = vi % 3
        style = "(thickness %g) (arrow_length %g) (text_position_mode %d) (arrow_direction %s) (extension_height 0.58642) (extension_offset %g) %s" % (
            0.1 + 0.05 * vi, 1.27 + 0.2 * vi, tpm, "inward" if vi % 2 else "outward", 0.5 * (vi % 3), "(keep_text_aligned yes)" if vi % 2 == 0 else "")
        add(dim("aligned", layer, [(x, yy), (x + 20 + vi, yy + vi)], "(height %g)" % (-3 if vi % 2 else 4), fmt, style, (x + 10, yy - 4)))
        add(dim("orthogonal", layer, [(x, yy + 8), (x + 15, yy + 11)], "(height %g) (orientation %d)" % (3, vi % 2), fmt, style, (x + 8, yy + 13)))
        add(dim("leader", layer, [(x + 24, yy + 8), (x + 28, yy + 12)], "", fmt, style.replace("(extension_height 0.58642)", "(text_frame %d)" % (vi % 3)), (x + 30, yy + 13)))
        add(dim("center", layer, [(x + 22, yy + 3), (x + 23.5, yy + 3)], "", fmt, "(thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_offset 0.5)", (x + 22, yy + 3)))
        add(dim("radial", layer, [(x + 5, yy + 15), (x + 8, yy + 17)], "(leader_length %g)" % (2 + vi), fmt, style.replace("(extension_height 0.58642)", ""), (x + 14, yy + 18)))
y = y + (k // 6 + 1) * 20 + 4
# tables
k = 0
for layer in ("F.SilkS", "B.SilkS", "F.Cu"):
    for vi in range(4):
        x = 10 + vi * 50; yy = y + k * 26
        cells = []
        ncol, nrow = 3, 3
        cw, rh = 12, 5
        for r in range(nrow):
            for c in range(ncol):
                span = ""
                if vi == 1 and r == 0 and c == 0: span = "(span 2 1)"
                if vi == 1 and r == 0 and c == 1: span = "(span 0 0)"
                if vi == 2 and r == 1 and c == 2: span = "(span 1 2)"
                if vi == 2 and r == 2 and c == 2: span = "(span 0 0)"
                mir = " mirror" if layer.startswith("B.") else ""
                cells.append('(table_cell "r%dc%d%s" (start %g %g) (end %g %g) (margins 0.4 0.4 0.4 0.4) %s (layer "%s") (uuid "%s") %s)' % (
                    r, c, "\\nx" if (r + c) % 3 == 0 else "", x + c * cw, yy + r * rh, x + (c + 1) * cw, yy + (r + 1) * rh, span, layer, uid(),
                    eff((1, 1), 0.15, r == 0, False, ("left top" if c == 0 else "right bottom" if c == 2 else "") + mir)))
        border = "(border (external %s) (header %s) (stroke (width %g) (type %s)))" % ("yes" if vi != 3 else "no", "yes" if vi % 2 == 0 else "no", 0.1 + 0.1 * vi, ["solid", "dash", "dot", "solid"][vi])
        seps = "(separators (rows %s) (cols %s) (stroke (width %g) (type %s)))" % ("yes" if vi < 2 else "no", "yes" if vi != 1 else "no", 0.1, ["solid", "solid", "dash_dot", "solid"][vi])
        add('(table (column_count 3) (layer "%s") %s %s (column_widths 12 12 12) (row_heights 5 5 5) (cells %s))' % (layer, border, seps, " ".join(cells)))
    k += 1
y += k * 26
out += "".join(S) + edge(0, 0, 215, y + 5) + ")\n"
sys.stdout.write(out)
