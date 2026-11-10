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
import os, sys, re
d = sys.argv[1]
def norm(t):
    out = []
    for l in t.split("\n"):
        if re.match(r'^(%TF\.(GenerationSoftware|CreationDate)|G04 Created by|G04 #@! TF\.(GenerationSoftware|CreationDate)|; DRILL file|; #@! TF\.(CreationDate|GenerationSoftware))', l) or l.strip().startswith(('"Version": "', '"CreationDate": "')): l = "<n>"
        out.append(l)
    return "\n".join(out)
res = []
for a, b in (("nat_g", "wasm/g"), ("nat_d", "wasm/d")):
    pa, pb = os.path.join(d, a), os.path.join(d, b)
    fa = sorted(os.listdir(pa)) if os.path.isdir(pa) else []; fb = sorted(os.listdir(pb)) if os.path.isdir(pb) else []
    rd = lambda p: norm(open(p, errors="replace").read())
    same = sum(1 for f in fa if f in fb and rd(os.path.join(pa, f)) == rd(os.path.join(pb, f)))
    diff = [f for f in fa if f in fb and rd(os.path.join(pa, f)) != rd(os.path.join(pb, f))]
    res.append("%s: native=%d wasm=%d same=%d diff=%s" % (a, len(fa), len(fb), same, diff[:4]))
print(" | ".join(res))
