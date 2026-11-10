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
"""effect.py KIND SETSFILE BOARD... : (reference only) number of files that differ from the default run, per set."""
import subprocess, sys, shlex, os, shutil, hashlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
kind, setsfile, boards = sys.argv[1], sys.argv[2], sys.argv[3:]
ROOT = "/home/user/kicad-source-mirror/rust/target/rev-fab/work/_effect"
REF = "/home/user/kicad-oracle/bin/kicad-cli"
def norm_line(b):
    for p in ("%TF.GenerationSoftware", "%TF.CreationDate", "G04 Created by KiCad", "G04 #@! TF.GenerationSoftware", "G04 #@! TF.CreationDate", "; DRILL file", "; #@! TF.CreationDate", "; #@! TF.GenerationSoftware"):
        if b.startswith(p): return "<n>"
    s = b.strip()
    if s.startswith('"Version": "') or s.startswith('"CreationDate": "'): return "<n>"
    return b
def runref(board, opts):
    stem = os.path.splitext(os.path.basename(board))[0]
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(ROOT + "/in"); os.makedirs(ROOT + "/out")
    for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru", ".kicad_prl"):
        p = os.path.join(os.path.dirname(board), stem + ext)
        if os.path.exists(p): shutil.copy(p, ROOT + "/in")
    env = dict(os.environ); env["KICAD_ORACLE_HOME"] = "/home/user/kicad-source-mirror/rust/target/rev-fab/work/_home"
    subprocess.run([REF, "pcb", "export", kind, "-o", ROOT + "/out/"] + opts + [ROOT + "/in/" + stem + ".kicad_pcb"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    r = {}
    for dp, dn, fn in os.walk(ROOT + "/out"):
        for f in fn:
            t = open(os.path.join(dp, f), "rb").read().decode("utf-8", "replace")
            r[f] = hashlib.md5("\n".join(norm_line(l) for l in t.split("\n")).encode()).hexdigest()
    return r
sets = []
for l in open(setsfile):
    l = l.rstrip("\n")
    if not l or l.startswith("#"): continue
    tag, _, opts = l.partition("|")
    sets.append((tag.strip(), shlex.split(opts)))
base = {b: runref(b, []) for b in boards}
for tag, opts in sets:
    cells = []
    for b in boards:
        r = runref(b, opts)
        nd = sum(1 for f in set(r) | set(base[b]) if r.get(f) != base[b].get(f))
        cells.append("%d" % nd)
    print("%-12s %s" % (tag, " ".join(cells)))
shutil.rmtree(ROOT, ignore_errors=True)
