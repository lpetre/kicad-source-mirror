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
"""REV-FAB: run reference kicad-cli and kicad-rs on COPIES of a board with one option set and
compare every output file (normalised) plus exit code and stdout.

  fabcmp.py [--keep] [--show N] KIND TAG BOARD -- OPTIONS...
KIND: gerbers | drill
Prints one line: RESULT|board|tag|rc ref/rs|same=N|diff=[..]|only_ref=[..]|only_rs=[..]|stdout
"""
import os, re, shutil, subprocess, sys, difflib

ROOT = "/home/user/kicad-source-mirror/rust/target/rev-fab/work"
REF = "/home/user/kicad-oracle/bin/kicad-cli"
RS = os.environ.get("KICAD_RS", "/home/user/kicad-source-mirror/rust/target/debug/kicad-rs")

def norm_line(l):
    b = l.rstrip("\r\n")
    if b.startswith("%TF.GenerationSoftware,KiCad,Pcbnew,"): return "<n>"
    if b.startswith("%TF.CreationDate,"): return "<n>"
    if b.startswith("G04 Created by KiCad (PCBNEW "): return "<n>"
    if b.startswith("G04 #@! TF.GenerationSoftware,"): return "<n>"
    if b.startswith("G04 #@! TF.CreationDate,"): return "<n>"
    if b.startswith("; DRILL file KiCad ") or b.startswith("; DRILL file {KiCad "): return "<n>"
    if b.startswith("; #@! TF.CreationDate,") or b.startswith("; #@! TF.GenerationSoftware,"): return "<n>"
    s = b.strip()
    if s.startswith('"Version": "') or s.startswith('"CreationDate": "'): return "<n>"
    if b.startswith("Drill report for ") or b.startswith("Created on "): return "<n>"
    if "<title>" in b or "date " in b and b.startswith("<desc>"): return "<n>"
    return b

def norm(data):
    try:
        t = data.decode("utf-8")
    except Exception:
        return data
    return "\n".join(norm_line(l) for l in t.split("\n"))

def run(cmd, env=None, timeout=600):
    try:
        p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, timeout=timeout)
        return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return 999, "", "TIMEOUT"

def main():
    a = sys.argv[1:]
    keep = False; show = 0
    while a and a[0].startswith("--"):
        if a[0] == "--keep": keep = True; a = a[1:]
        elif a[0] == "--show": show = int(a[1]); a = a[2:]
        else: break
    kind, tag, board = a[0], a[1], a[2]
    opts = a[4:] if len(a) > 3 and a[3] == "--" else []
    stem = os.path.splitext(os.path.basename(board))[0]
    base = os.path.join(ROOT, stem + "__" + tag)
    shutil.rmtree(base, ignore_errors=True)
    res = {}
    for side in ("ref", "rs"):
        d = os.path.join(base, side); os.makedirs(os.path.join(d, "in")); os.makedirs(os.path.join(d, "out"))
        src = os.path.dirname(board)
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru", ".kicad_prl", ".kicad_wks"):
            p = os.path.join(src, stem + ext)
            if os.path.exists(p): shutil.copy(p, os.path.join(d, "in"))
        inp = os.path.join(d, "in", stem + ".kicad_pcb")
        out = os.path.join(d, "out") + "/"
        o2 = [x.replace("@OUT@", out) for x in opts]
        if side == "ref":
            env = dict(os.environ); env["KICAD_ORACLE_HOME"] = os.path.join(ROOT, "_home")
            rc, so, se = run([REF, "pcb", "export", kind, "-o", out] + o2 + [inp], env)
        else:
            rc, so, se = run([RS, "pcb", "export", kind, "-o", out] + o2 + [inp])
        so = so.replace(d, "<D>")
        se = "\n".join(l for l in se.replace(d, "<D>").split("\n") if "api.v1.schema.json" not in l and l.strip())
        files = {}
        for dp, dn, fn in os.walk(os.path.join(d, "out")):
            for f in fn:
                p = os.path.join(dp, f)
                files[os.path.relpath(p, os.path.join(d, "out"))] = norm(open(p, "rb").read())
        res[side] = (rc, so, se, files)
    rr, rs = res["ref"], res["rs"]
    same = [f for f in rr[3] if f in rs[3] and rr[3][f] == rs[3][f]]
    diff = [f for f in rr[3] if f in rs[3] and rr[3][f] != rs[3][f]]
    only_ref = [f for f in rr[3] if f not in rs[3]]
    only_rs = [f for f in rs[3] if f not in rr[3]]
    so_same = "stdout=same" if rr[1] == rs[1] else "stdout=DIFF"
    ok = (not diff and not only_ref and not only_rs and rr[0] == rs[0] and rr[1] == rs[1])
    print("%s|%s|%s|rc %d/%d|same=%d|diff=%s|only_ref=%s|only_rs=%s|%s" % (
        "OK  " if ok else "FAIL", stem, tag, rr[0], rs[0], len(same), diff, only_ref, only_rs, so_same))
    if not ok and show:
        if rr[1] != rs[1]:
            print("  --- stdout ref:\n   " + "\n   ".join(rr[1].split("\n")[:show]))
            print("  --- stdout rs:\n   " + "\n   ".join(rs[1].split("\n")[:show]))
        if rs[2]: print("  --- stderr rs: " + "\n   ".join(rs[2].split("\n")[-6:]))
        if rr[2] and rr[0] != 0: print("  --- stderr ref: " + "\n   ".join(rr[2].split("\n")[-6:]))
        for f in diff[:3]:
            x, y = rr[3][f], rs[3][f]
            if isinstance(x, bytes) or isinstance(y, bytes): print("  binary diff", f); continue
            dl = list(difflib.unified_diff(x.split("\n"), y.split("\n"), "ref/" + f, "rs/" + f, n=1, lineterm=""))
            print("  " + "\n  ".join(dl[:show]))
            print("  (%d diff lines)" % len(dl))
    if ok and not keep:
        shutil.rmtree(base, ignore_errors=True)
    elif not keep:
        # keep only inputs + differing files
        for side in ("ref", "rs"):
            for f in same:
                try: os.remove(os.path.join(base, side, "out", f))
                except OSError: pass

main()
