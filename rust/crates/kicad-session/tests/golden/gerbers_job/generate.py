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
"""Regenerates the FAB-07 `kicad-cli pcb export gerbers` job goldens with the reference
kicad-cli (docs/ORACLE.md), always on copies in a scratch directory.

    python3 generate.py

Every .kicad_pcb under qa/data and demos smaller than 300 KB (with its .kicad_pro /
.kicad_dru), for each option set of SETS: `kicad-cli pcb export gerbers <set> -o <out>/ <board>`.
Written to corpus.txt (compact): `B <set> <exit code> <stdout hash> <board path relative to the
KiCad tree>`, then one line per file in "Plotted to" order with the first 8 hex digits of the
FNV-1a 64 of the normalised file (the file names and their order are covered by the stdout
hash: stdout with the output directory replaced by `out/`). The Gerber job file (.gbrjob) is
not hashed (FAB-09).

Normalisation (docs/analysis/plot-fab.md 9.3): the lines carrying the version and the date are
replaced by `<normalised>` (as kicad-fab's tests/golden/plot_driver/generate.py).
"""
import os
import re
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../../.."))
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
SCRATCH = os.environ.get("SCRATCH", os.path.join(KICAD, "rust/target/session-gerbers-job-golden"))

SETS = [
    ("default", []),
    ("nox2_drill", ["--no-x2", "--use-drill-file-origin"]),
]


def normalise_line(line):
    if line.startswith("%TF.GenerationSoftware,KiCad,Pcbnew,") and line.endswith("*%"):
        return "<normalised>"
    if line.startswith("%TF.CreationDate,") and line.endswith("*%"):
        return "<normalised>"
    if line.startswith("G04 Created by KiCad (PCBNEW ") and ") date " in line and line.endswith("*"):
        return "<normalised>"
    if line.startswith("G04 #@! TF.GenerationSoftware,") and line.endswith("*"):
        return "<normalised>"
    if line.startswith("G04 #@! TF.CreationDate,") and line.endswith("*"):
        return "<normalised>"
    return line


def normalise(text):
    return "".join(normalise_line(l[:-1] if l.endswith("\n") else l) + ("\n" if l.endswith("\n") else "")
                   for l in text.splitlines(keepends=True))


def fnv(text):
    h = 0xcbf29ce484222325
    for b in text.encode("utf-8"):
        h ^= b
        h = (h * 0x100000001b3) & 0xffffffffffffffff
    return "%016x" % h


def run(board, args, work):
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(os.path.join(work, "in"))
    os.makedirs(os.path.join(work, "out"))
    shutil.copy(board, os.path.join(work, "in"))
    stem = os.path.splitext(board)[0]
    for ext in (".kicad_pro", ".kicad_dru"):
        if os.path.exists(stem + ext):
            shutil.copy(stem + ext, os.path.join(work, "in"))
    env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(SCRATCH, "home"))
    inp = os.path.join(work, "in", os.path.basename(board))
    out_dir = os.path.join(work, "out") + "/"
    res = subprocess.run([CLI, "pcb", "export", "gerbers", *args, "-o", out_dir, inp],
                         env=env, capture_output=True, text=True, timeout=600)
    files = re.findall(r"Plotted to '([^']*)'", res.stdout)
    hashes = []
    for f in files:
        with open(f, encoding="utf-8", newline="") as fh:
            hashes.append(fnv(normalise(fh.read()))[:8])
    stdout = res.stdout.replace(out_dir, "out/")
    shutil.rmtree(work, ignore_errors=True)
    return res.returncode, fnv(stdout)[:8], hashes


def main():
    os.makedirs(SCRATCH, exist_ok=True)
    boards = []
    for top in ("qa/data", "demos"):
        for root, _, files in os.walk(os.path.join(KICAD, top)):
            for f in files:
                p = os.path.join(root, f)
                if f.endswith(".kicad_pcb") and os.path.getsize(p) < 300 * 1024:
                    boards.append(os.path.relpath(p, KICAD))
    lines = []
    for rel in sorted(boards):
        for set_name, args in SETS:
            code, out_hash, hashes = run(os.path.join(KICAD, rel), args, os.path.join(SCRATCH, "w"))
            lines.append("B %s %d %s %s\n" % (set_name, code, out_hash, rel))
            lines.extend("%s\n" % h for h in hashes)
    with open(os.path.join(HERE, "corpus.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(lines))


if __name__ == "__main__":
    main()
