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
"""Regenerates the FAB-12 goldens (drill maps) with the reference kicad-cli (docs/ORACLE.md),
always on copies in a scratch directory.

    python3 generate.py

Every .kicad_pcb under qa/data and demos smaller than 300 KB (with its .kicad_pro / .kicad_dru),
then the crafted boards of EXTRA_DIRS, for each option set of SETS:
`kicad-cli pcb export drill --generate-map <set> -o <out>/ <board>`.

Written to corpus.txt (compact): `B <board path relative to the KiCad tree>`, then per set
`S <set> <exit code>`, then one line `F <file name> <hash>` per "Created file" line of stdout naming a drill map
(`*-drl_map.*`), in order, where <hash>
is the first 8 hex digits of the FNV-1a 64 of the normalised file. Boards for which kicad-cli
created no file have no F lines. For the crafted boards (EXTRA_DIRS) the normalised files are
also stored in full
under <board stem>/<set>/ for the pairs of KEEP.

Normalisation (docs/analysis/plot-fab.md 9.3): the lines carrying the version and the date are
replaced by `<normalised>` (same rule as kicad-session's gerbers_job golden); in SVG maps the
date of the `<title>` line is replaced by `<date>`.
"""
import os
import re
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../../.."))
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
SCRATCH = os.environ.get("SCRATCH", os.path.join(KICAD, "rust/target/fab-drill-map-golden"))

# Crafted boards (relative to the KiCad tree), after the corpus boards.
EXTRA_DIRS = [
    "rust/crates/kicad-fab/tests/data/drill_map",
    "rust/crates/kicad-fab/tests/data/excellon_corpus",
]

# (crafted board stem, option set) whose full normalised maps are kept (the maps are large:
# every legend character is stroked; the others are compared by hash only).
KEEP = [("map_tiny", "exc_gbr_sep"), ("map_tiny", "exc_svg_sep")]

SETS = [
    ("exc_gbr", ["--map-format", "gerberx2"]),
    ("exc_gbr_sep", ["--map-format", "gerberx2", "--excellon-separate-th"]),
    ("exc_gbr_plot", ["--map-format", "gerberx2", "--drill-origin", "plot"]),
    ("exc_svg", ["--map-format", "svg"]),
    ("exc_svg_sep", ["--map-format", "svg", "--excellon-separate-th"]),
    ("gbr_gbr", ["--format", "gerber", "--map-format", "gerberx2"]),
    ("gbr_svg", ["--format", "gerber", "--map-format", "svg"]),
]

SVG_TITLE = re.compile(r"^(<title>SVG Image created as .* date )\S+( </title>)$")


def normalise_line(line):
    if line.startswith("%TF.GenerationSoftware,KiCad,Pcbnew,") and line.endswith("*%"):
        return "<normalised>"
    if line.startswith("%TF.CreationDate,") and line.endswith("*%"):
        return "<normalised>"
    if line.startswith("G04 Created by KiCad (PCBNEW ") and ") date " in line and line.endswith("*"):
        return "<normalised>"
    m = SVG_TITLE.match(line)
    if m:
        return m.group(1) + "<date>" + m.group(2)
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


def run(board, args, work, keep_dir=None):
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
    res = subprocess.run([CLI, "pcb", "export", "drill", "--generate-map", *args, "-o", out_dir, inp],
                         env=env, capture_output=True, text=True, timeout=600)
    files = [f for f in re.findall(r"Created file '([^']*)'", res.stdout) if "-drl_map." in f]
    lines = []
    if keep_dir:
        shutil.rmtree(keep_dir, ignore_errors=True)
        os.makedirs(keep_dir)
    for f in files:
        with open(f, encoding="utf-8", newline="") as fh:
            text = normalise(fh.read())
        name = os.path.relpath(f, out_dir)
        lines.append("F %s %s" % (name, fnv(text)[:8]))
        if keep_dir:
            with open(os.path.join(keep_dir, name), "w", encoding="utf-8", newline="") as fh:
                fh.write(text)
    shutil.rmtree(work, ignore_errors=True)
    return res.returncode, lines


def main():
    os.makedirs(SCRATCH, exist_ok=True)
    boards = []
    for top in ("qa/data", "demos"):
        for root, _, files in os.walk(os.path.join(KICAD, top)):
            for f in files:
                p = os.path.join(root, f)
                if f.endswith(".kicad_pcb") and os.path.getsize(p) < 300 * 1024:
                    boards.append((os.path.relpath(p, KICAD), False))
    boards.sort()
    for d in EXTRA_DIRS:
        boards.extend((os.path.join(d, f), True) for f in sorted(os.listdir(os.path.join(KICAD, d)))
                      if f.endswith(".kicad_pcb"))
    only = os.environ.get("ONLY")  # debugging: restrict to boards whose path contains ONLY
    if only:
        boards = [b for b in boards if only in b[0]]
    out = []
    for rel, crafted in boards:
        for set_name, args in SETS:
            keep = None
            stem = os.path.splitext(os.path.basename(rel))[0]
            if crafted and (stem, set_name) in KEEP:
                keep = os.path.join(HERE, stem, set_name)
            code, lines = run(os.path.join(KICAD, rel), args, os.path.join(SCRATCH, "w"), keep)
            if set_name == SETS[0][0]:
                out.append("B %s\n" % rel)
            out.append("S %s %d\n" % (set_name, code))
            out.extend("%s\n" % l for l in lines)
    with open(os.environ.get("OUT", os.path.join(HERE, "corpus.txt")), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(out))


if __name__ == "__main__":
    main()
