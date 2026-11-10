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
"""effect_bp.py CRAFTDIR ORIGBOARD : reference only; files differing between ORIG --board-plot-params and each bp_* variant."""
import os, sys, subprocess, shutil, hashlib, glob
craft, orig = sys.argv[1], sys.argv[2]
stem = os.path.splitext(os.path.basename(orig))[0]
ROOT = "/home/user/kicad-source-mirror/rust/target/rev-fab/work/_effect"
def norm_line(b):
    for p in ("%TF.GenerationSoftware", "%TF.CreationDate", "G04 Created by KiCad", "G04 #@! TF.GenerationSoftware", "G04 #@! TF.CreationDate"):
        if b.startswith(p): return "<n>"
    s = b.strip()
    if s.startswith('"Version": "') or s.startswith('"CreationDate": "'): return "<n>"
    return b
def runref(board):
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(ROOT + "/in"); os.makedirs(ROOT + "/out")
    for ext in (".kicad_pcb", ".kicad_pro"):
        p = os.path.join(os.path.dirname(board), stem + ext)
        if os.path.exists(p): shutil.copy(p, ROOT + "/in")
    env = dict(os.environ); env["KICAD_ORACLE_HOME"] = "/home/user/kicad-source-mirror/rust/target/rev-fab/work/_home"
    subprocess.run(["/home/user/kicad-oracle/bin/kicad-cli", "pcb", "export", "gerbers", "--board-plot-params", "-o", ROOT + "/out/", ROOT + "/in/" + stem + ".kicad_pcb"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    r = {}
    for dp, dn, fn in os.walk(ROOT + "/out"):
        for f in fn:
            t = open(os.path.join(dp, f), "rb").read().decode("utf-8", "replace")
            r[os.path.relpath(os.path.join(dp, f), ROOT + "/out")] = hashlib.md5("\n".join(norm_line(l) for l in t.split("\n")).encode()).hexdigest()
    return r
base = runref(orig)
out = []
for d in sorted(glob.glob(craft + "/bp_*")):
    b = os.path.join(d, stem + ".kicad_pcb")
    if not os.path.exists(b): continue
    r = runref(b)
    nd = sum(1 for f in set(r) | set(base) if r.get(f) != base.get(f))
    out.append("%s=%d/%d" % (os.path.basename(d)[3:], nd, len(r)))
print(stem, " ".join(out))
shutil.rmtree(ROOT, ignore_errors=True)
