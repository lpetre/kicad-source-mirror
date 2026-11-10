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
"""outcmp.py BOARD KIND -- ARGS... : run both tools with cwd = the board copy's directory; compare the resulting file trees."""
import os, shutil, subprocess, sys, hashlib, re
board, kind = sys.argv[1], sys.argv[2]
args = sys.argv[4:]
stem = os.path.splitext(os.path.basename(board))[0]
ROOT = "/home/user/kicad-source-mirror/rust/target/rev-fab/work/_outcmp"
res = {}
for side, exe in (("ref", "/home/user/kicad-oracle/bin/kicad-cli"), ("rs", os.environ["KICAD_RS"])):
    d = os.path.join(ROOT, side)
    shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
    for ext in (".kicad_pcb", ".kicad_pro"):
        p = os.path.join(os.path.dirname(board), stem + ext)
        if os.path.exists(p): shutil.copy(p, d)
    env = dict(os.environ); env["KICAD_ORACLE_HOME"] = "/home/user/kicad-source-mirror/rust/target/rev-fab/work/_home"
    p = subprocess.run([exe, "pcb", "export", kind] + args + [stem + ".kicad_pcb"], cwd=d, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    files = {}
    for dp, dn, fn in os.walk(d):
        for f in fn:
            if f.endswith((".kicad_pcb", ".kicad_pro", ".kicad_prl", ".lck")): continue
            t = open(os.path.join(dp, f), "rb").read().decode("utf-8", "replace")
            t = "\n".join(l for l in t.split("\n") if not re.search(r"CreationDate|GenerationSoftware|Created by KiCad|DRILL file|\"Version\"|Drill report|Created on", l))
            files[os.path.relpath(os.path.join(dp, f), d)] = hashlib.md5(t.encode()).hexdigest()
    so = p.stdout.decode("utf-8", "replace").replace(d, "<D>")
    se = "\n".join(l for l in p.stderr.decode("utf-8", "replace").replace(d, "<D>").split("\n") if "schema" not in l and l.strip())
    res[side] = (p.returncode, files, so, se)
a, b = res["ref"], res["rs"]
ok = a[0] == b[0] and a[1] == b[1] and a[2] == b[2]
print("%s %-40s rc %d/%d files %d/%d stdout %s" % ("OK  " if ok else "FAIL", " ".join(args)[:40], a[0], b[0], len(a[1]), len(b[1]), "same" if a[2] == b[2] else "DIFF"))
if not ok:
    print("   ref files:", sorted(a[1])[:6]); print("   rs  files:", sorted(b[1])[:6])
    if a[2] != b[2]: print("   ref out:", a[2][:300].replace("\n", " | ")); print("   rs  out:", b[2][:300].replace("\n", " | "))
    if b[3] or a[3]: print("   ref err:", a[3][-250:].replace("\n", " | ")); print("   rs  err:", b[3][-250:].replace("\n", " | "))
shutil.rmtree(ROOT, ignore_errors=True)
