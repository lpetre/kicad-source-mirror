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
"""mutate.py BOARD OUTDIR TAG key=value... [@dnp=N] [@text=OLD=>NEW]
Writes OUTDIR/TAG/<stem>.kicad_pcb with the pcbplotparams entries replaced/added (and N footprints marked DNP)."""
import os, re, shutil, sys
board, outdir, tag = sys.argv[1:4]
stem = os.path.splitext(os.path.basename(board))[0]
t = open(board, encoding="utf-8").read()
m = re.search(r"\(pcbplotparams\b", t)
start = m.start()
# find matching close
depth = 0; i = start
while True:
    c = t[i]
    if c == '"':
        i += 1
        while t[i] != '"':
            if t[i] == '\\': i += 1
            i += 1
    elif c == '(': depth += 1
    elif c == ')':
        depth -= 1
        if depth == 0: break
    i += 1
block = t[start:i + 1]
dnp = 0
for kv in sys.argv[4:]:
    if kv.startswith("@dnp="):
        dnp = int(kv[5:]); continue
    if kv.startswith("@text="):
        old, new = kv[6:].split("=>"); assert old in t, old; t = t.replace(old, new); continue
    k, v = kv.split("=", 1)
    pat = re.compile(r"\(" + re.escape(k) + r"\s[^()]*\)")
    if v == "@del":
        block = pat.sub("", block)
    elif pat.search(block):
        block = pat.sub("(%s %s)" % (k, v), block, count=1)
    else:
        block = block[:-1] + "\t(%s %s)\n\t\t)" % (k, v)
m = re.search(r"\(pcbplotparams\b", t); start = m.start()
depth = 0; i = start
while True:
    c = t[i]
    if c == '"':
        i += 1
        while t[i] != '"':
            if t[i] == '\\': i += 1
            i += 1
    elif c == '(': depth += 1
    elif c == ')':
        depth -= 1
        if depth == 0: break
    i += 1
t = t[:start] + block + t[i + 1:]
if dnp:
    # mark every 3rd footprint DNP up to N: add/extend attr
    parts = re.split(r"(?m)^(\t\(footprint )", t)
    out = [parts[0]]; n = 0; idx = 0
    for j in range(1, len(parts), 2):
        body = parts[j + 1]
        if idx % 3 == 0 and n < dnp:
            if re.search(r"(?m)^\t\t\(attr [^)]*\)", body):
                body = re.sub(r"(?m)^(\t\t\(attr [^)]*)\)", lambda mm: mm.group(1) + (" dnp)" if " dnp" not in mm.group(1) else ")"), body, count=1)
            else:
                body = re.sub(r"(?m)^(\t\t\(property )", "\t\t(attr dnp)\n\\1", body, count=1)
            n += 1
        idx += 1
        out.append(parts[j]); out.append(body)
    t = "".join(out)
d = os.path.join(outdir, tag); os.makedirs(d, exist_ok=True)
open(os.path.join(d, stem + ".kicad_pcb"), "w", encoding="utf-8").write(t)
for ext in (".kicad_pro", ".kicad_dru"):
    p = os.path.join(os.path.dirname(board), stem + ext)
    if os.path.exists(p): shutil.copy(p, d)
print(os.path.join(d, stem + ".kicad_pcb"))
