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
import sys, re, os, subprocess
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
# one_pad.py OUT fprot sx sy padrot shape
out, fprot, sx, sy, padrot, shape = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4], float(sys.argv[5]), sys.argv[6]
smd = '(layers "F.Cu" "F.Mask" "F.Paste")'
body = {"cham": "smd roundrect @AT@ (size 2.2 1.4) %s (roundrect_rratio 0) (chamfer_ratio 0.2) (chamfer top_left bottom_right)" % smd,
        "trap": "smd trapezoid @AT@ (size 2 1.5) (rect_delta 0.4 0) %s (solder_mask_margin 0.1)" % smd,
        "cust": "smd custom @AT@ (size 0.8 0.8) %s (options (clearance outline) (anchor rect)) (primitives (gr_poly (pts (xy -1 -0.6) (xy 1.2 -0.9) (xy 0.8 0.7) (xy -0.9 0.5)) (width 0.1) (fill yes)))" % smd,
        "rrect": "smd roundrect @AT@ (size 2.2 1.4) %s (roundrect_rratio 0.25)" % smd,
        "rect": "smd rect @AT@ (size 2.2 1.4) %s" % smd,
        "oval": "smd oval @AT@ (size 2.2 1.4) %s" % smd}[shape]
a = fprot + padrot
s = header(LAYERS2)
s += ' (footprint "rev:one" (layer "F.Cu") (uuid "%s") (at 135 30 %g)\n  (attr smd)\n' % (uid(), fprot)
s += '  (pad "1" %s (uuid "%s"))\n )\n' % (body.replace("@AT@", "(at -10 6 %g)" % a), uid())
s += edge(100, 0, 170, 60) + ")\n"
os.makedirs(os.path.dirname(out), exist_ok=True)
open(out, "w").write(s)
env = dict(os.environ); env["KICAD_ORACLE_HOME"] = "/home/user/kicad-source-mirror/rust/target/rev-fab/work/_home"
subprocess.run(["/home/user/kicad-oracle/bin/kicad-cli", "pcb", "upgrade", "--force", out], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
t = open(out).read()
t2 = re.sub(r"\(scale 1 1\)", "(scale %s %s)" % (sx, sy), t, count=1)
open(out, "w").write(t2)
subprocess.run(["/home/user/kicad-oracle/bin/kicad-cli", "pcb", "upgrade", "--force", out], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
