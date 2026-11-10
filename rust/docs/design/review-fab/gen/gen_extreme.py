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
"""Crafted board: extreme / odd coordinates and sizes (negative, far, nm-odd), slots at odd angles."""
import os, sys
from common import *
# REV_FAR=1: coordinates up to 1.6 m (finding X-1: beyond 2^30 nm); default: everything below 1073 mm
FAR = bool(os.environ.get("REV_FAR"))
out = header(LAYERS4)
S = []
def add(s): S.append(" " + s + "\n")
pts = [(-500, -300), (-0.000001, 0.000001), (0, 0), ((1499.999999, 1500.000001) if FAR else (1049.999999, 1050.000001)), (123.456789, 98.765432), (0.0005, 0.0015), (100.0005, 150.0005),
       (25.4, 2.54), (25.40001, 2.53999), (-234.5678, 987.6543), (999.9995, 0.0005), ((1e-6, 1600) if FAR else (1e-6, 1060)), (300.123456, -200.654321)]
sizes = [(0.8, 0.4), (0.000002, 0.000001), (100, 99.999999), (0.123457, 0.061729), (0.6, 0.3), (1.0005, 0.5005), (0.635, 0.3175), (0.1016, 0.0508)]
k = 0
for i, (x, y) in enumerate(pts):
    for j, (sz, dr) in enumerate(sizes):
        if sz > 50 and i not in (2, 4): continue
        xx = x + (j % 4) * 0.7001; yy = y + (j // 4) * 0.9003
        add('(via (at %.6f %.6f) (size %.6f) (drill %.6f) (layers "F.Cu" "B.Cu") (net %d) (uuid "%s"))' % (xx, yy, sz, dr, 1 + (k % 3), uid())); k += 1
    add('(segment (start %.6f %.6f) (end %.6f %.6f) (width %.6f) (layer "F.Cu") (net 1) (uuid "%s"))' % (x, y, x + 3.000001, y - 2.999999, 0.123457, uid()))
    add('(gr_circle (center %.6f %.6f) (end %.6f %.6f) (stroke (width 0.100001) (type dash_dot)) (fill none) (layer "F.SilkS") (uuid "%s"))' % (x, y, x + 1.000001, y, uid()))
tht = '(layers "*.Cu" "*.Mask")'
for fi, (fx, fy, rot) in enumerate(((50, 50, 33.3), (-200.5, 77.7, 271.25), ((1400.123 if FAR else 1040.123), -100.001, 0.01), (80, 80, 180))):
    s = ' (footprint "rev:x%d" (layer "F.Cu") (uuid "%s") (at %.6f %.6f %g)\n  (attr through_hole)\n' % (fi, uid(), fx, fy, rot)
    n = 1
    for pi, (prot, w, h, dw, dh, ox, oy) in enumerate(((0, 2.4, 1.4, 1.4, 0.7, 0, 0), (17.5, 1.4, 3.000001, 0.700001, 2.2, 0, 0), (90, 3, 1.5, 2.000001, 0.999999, 0.25, -0.1),
                                                        (45, 2, 2, 1.000001, 1, 0, 0), (-33.33, 5, 1.2, 4.5, 0.4, 0, 0.3), (180, 1.5, 1.5, 0.5, 0.500001, 0, 0), (0, 0.9, 0.9, 0.3, 0.9, 0, 0))):
        px = -9 + pi * 3.3333; py = (pi % 2) * 2.2222
        a = rot + prot
        s += '  (pad "%d" thru_hole oval (at %.6f %.6f %g) (size %.6f %.6f) (drill oval %.6f %.6f%s) %s (net %d "%s") (uuid "%s"))\n' % (
            n, px, py, a, w, h, dw, dh, " (offset %g %g)" % (ox, oy) if ox or oy else "", tht, 1 + pi % 3, ["N1", "N2", "GND"][pi % 3], uid()); n += 1
        s += '  (pad "" np_thru_hole oval (at %.6f %.6f %g) (size %.6f %.6f) (drill oval %.6f %.6f) %s (uuid "%s"))\n' % (px, py + 6.1, a + 11, dw, dh, dw, dh, tht, uid())
    s += " )\n"
    S.append(s)
out += "".join(S) + (edge(-510, -310, 1560, 1610) if FAR else edge(-510, -310, 1065, 1065)) + ")\n"
sys.stdout.write(out)
