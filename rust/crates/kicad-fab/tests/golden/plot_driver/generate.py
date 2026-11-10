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
"""Regenerates the FAB-03 plot driver goldens with the reference kicad-cli (docs/ORACLE.md).

    python3 generate.py [crafted|corpus|all]

crafted: every board of tests/data/plot_driver (with its .kicad_pro, if any) x the option
  sets of SETS (`kicad-cli pcb export gerbers <set> -o out <board>` on a copy in a scratch
  dir). Written to crafted.txt: `#case <board stem> <set>`, then per produced file in the
  "Plotted to" order `#file <file name>` followed by the normalised file text.
corpus: every .kicad_pcb under qa/data and demos smaller than 300 KB, CLI defaults. Written
  to corpus.txt (compact, to keep the golden small): `B <board path relative to the KiCad
  tree>`, then one line per produced file in "Plotted to" order:
  `<name hash> <header hash> <file hash>`, each the first 8 hex digits of the FNV-1a 64 of
  the file name, of the normalised header (the text up to and including
  `G04 APERTURE LIST*`) and of the normalised file; no file lines when kicad-cli produced
  nothing.

Normalisation (docs/analysis/plot-fab.md 9.3): the lines carrying the version and the date
are replaced by `<normalised>`; tests/plot_driver.rs applies the same rules.
"""
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../../.."))
CRATE = os.path.abspath(os.path.join(HERE, "../../.."))
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
SCRATCH = os.environ.get("SCRATCH", os.path.join(KICAD, "rust/target/fab-plot-driver-golden"))

LAYERS = "F.Cu,In1.Cu,B.Cu,F.Mask,F.Paste,F.SilkS,Edge.Cuts,User.1"
SETS = [
    ("default", []),
    ("no_x2", ["--no-x2", "-l", LAYERS]),
    ("prec5", ["--precision", "5", "-l", LAYERS]),
    ("no_protel", ["--no-protel-ext", "-l", LAYERS]),
    ("drill_origin", ["--use-drill-file-origin", "-l", LAYERS]),
    ("board_params", ["--board-plot-params"]),
    ("common", ["-l", "F.Cu,B.Cu", "--cl", "Edge.Cuts,F.Fab"]),
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


def header(text):
    mark = "G04 APERTURE LIST*\n"
    i = text.find(mark)
    return text if i < 0 else text[: i + len(mark)]


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
    res = subprocess.run([CLI, "pcb", "export", "gerbers", *args, "-o", os.path.join(work, "out") + "/", inp],
                         env=env, capture_output=True, text=True, timeout=600)
    files = re.findall(r"Plotted to '([^']*)'", res.stdout)
    out = []
    for f in files:
        with open(f, encoding="utf-8", newline="") as fh:
            out.append((os.path.basename(f), normalise(fh.read())))
    shutil.rmtree(work, ignore_errors=True)
    return out


def crafted():
    data = os.path.join(CRATE, "tests/data/plot_driver")
    lines = []
    for name in sorted(os.listdir(data)):
        if not name.endswith(".kicad_pcb"):
            continue
        stem = name[: -len(".kicad_pcb")]
        for set_name, args in SETS:
            lines.append("#case %s %s\n" % (stem, set_name))
            for fname, text in run(os.path.join(data, name), args, os.path.join(SCRATCH, "w")):
                lines.append("#file %s\n" % fname)
                lines.append(text)
    with open(os.path.join(HERE, "crafted.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(lines))


def corpus():
    boards = []
    for top in ("qa/data", "demos"):
        for root, _, files in os.walk(os.path.join(KICAD, top)):
            for f in files:
                p = os.path.join(root, f)
                if f.endswith(".kicad_pcb") and os.path.getsize(p) < 300 * 1024:
                    boards.append(os.path.relpath(p, KICAD))
    lines = []
    for rel in sorted(boards):
        out = run(os.path.join(KICAD, rel), [], os.path.join(SCRATCH, "w"))
        lines.append("B %s\n" % rel)
        for fname, text in out:
            lines.append("%s %s %s\n" % (fnv(fname)[:8], fnv(header(text))[:8], fnv(text)[:8]))
    with open(os.path.join(HERE, "corpus.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(lines))


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "all"
    os.makedirs(SCRATCH, exist_ok=True)
    if what in ("crafted", "all"):
        crafted()
    if what in ("corpus", "all"):
        corpus()
