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
"""matrix.py KIND SETSFILE BOARD... : run every option set (lines `tag<TAB or |>opts`) on every board."""
import subprocess, sys, shlex, os
kind, setsfile, boards = sys.argv[1], sys.argv[2], sys.argv[3:]
T = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fabcmp.py")
sets = []
for l in open(setsfile):
    l = l.rstrip("\n")
    if not l or l.startswith("#"): continue
    tag, _, opts = l.partition("|")
    sets.append((tag.strip(), shlex.split(opts)))
ok = 0; fail = 0; show = os.environ.get("SHOW", "0")
for tag, opts in sets:
    nok = 0; fl = []
    for b in boards:
        p = subprocess.run([sys.executable, T, "--show", show, kind, tag, b, "--"] + opts, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        out = p.stdout.decode("utf-8", "replace")
        first = out.split("\n")[0]
        if first.startswith("OK"):
            nok += 1; ok += 1
        else:
            fail += 1; fl.append(out.rstrip()[:int(os.environ.get("MAXC", "700"))])
    print("SET %-14s %-60s ok=%d fail=%d" % (tag, " ".join(opts)[:60], nok, len(fl)))
    for f in fl: print("  " + f.replace("\n", "\n  "))
    sys.stdout.flush()
print("TOTAL ok=%d fail=%d" % (ok, fail))
