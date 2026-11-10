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
"""Regenerates the FAB-11 `kicad-cli pcb export svg` job goldens with the reference kicad-cli
(docs/ORACLE.md), always on copies in a scratch directory.

    SCRATCH=/tmp/x python3 generate.py

Every .kicad_pcb under qa/data and demos smaller than 300 KB (with its .kicad_pro /
.kicad_dru) and the crafted boards of EXTRA_DIRS with the set `default`; the five boards of
OPTION_BOARDS with every other set of SETS. Each run is
`kicad-cli pcb export svg <set> -o <out> <board>` (`<out>` = `out/<board name>.svg` in single
mode, `out/` otherwise). Written to corpus.txt (compact): `B <set> <exit code> <stdout hash>
<board path relative to the KiCad tree>`, then one line per file in "Plotted to" order with the
first 8 hex digits of the FNV-1a 64 of the normalised file (the file names and their order are
covered by the stdout hash: stdout with the output directory replaced by `out/`).

Normalisation (docs/analysis/plot-fab.md 9.3): the date of the `<title>` line
(`<title>SVG Image created as NAME date DATE </title>`) is replaced by `<normalised>`.
"""
import os
import re
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../../.."))
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
SCRATCH = os.environ.get("SCRATCH", os.path.join(KICAD, "rust/target/session-svg-job-golden"))

# Crafted boards (relative to the KiCad tree), after the corpus boards.
EXTRA_DIRS = [
    "rust/crates/kicad-fab/tests/data/plot_driver",
    "rust/crates/kicad-fab/tests/data/plot_items",
    "rust/crates/kicad-fab/tests/data/plot_graphics",
    "rust/crates/kicad-fab/tests/data/plot_layer_poly",
    "rust/crates/kicad-fab/tests/data/plot_text",
    "rust/crates/kicad-fab/tests/data/svg_plot",
]

# The boards of the option sets other than `default`.
OPTION_BOARDS = [
    "demos/ecc83/ecc83-pp.kicad_pcb",
    "demos/microwave/microwave.kicad_pcb",
    "rust/crates/kicad-fab/tests/data/plot_items/pads.kicad_pcb",
    "rust/crates/kicad-fab/tests/data/plot_graphics/graphics.kicad_pcb",
    "rust/crates/kicad-fab/tests/data/svg_plot/svg_marks.kicad_pcb",
]

ALL = "F.Cu,In1.Cu,B.Cu,F.Paste,B.Paste,F.SilkS,B.SilkS,F.Mask,B.Mask,Edge.Cuts,F.Fab,B.Fab," \
      "F.CrtYd,User.Drawings,User.Comments,User.1"
BASE = ["--mode-multi", "--exclude-drawing-sheet", "-l",
        "F.Cu,B.Cu,F.Mask,F.SilkS,B.SilkS,Edge.Cuts,F.Fab"]

SETS = [
    ("default", ["--mode-multi", "--exclude-drawing-sheet", "-l", ALL]),
    ("single", ["--mode-single", "--exclude-drawing-sheet", "-l", "F.Cu,B.Cu,F.SilkS,Edge.Cuts"]),
    ("single_fit", ["--mode-single", "--exclude-drawing-sheet", "--fit-page-to-board", "-l",
                    "B.Cu,F.Fab"]),
    ("common", ["--mode-multi", "--exclude-drawing-sheet", "-l", "F.Cu,B.Cu", "--cl",
                "Edge.Cuts,F.Fab"]),
    ("bw", BASE + ["--black-and-white"]),
    ("negative", BASE + ["--negative"]),
    ("mirror", BASE + ["--mirror"]),
    ("drill0", BASE + ["--drill-shape-opt", "0"]),
    ("drill1", BASE + ["--drill-shape-opt", "1"]),
    ("psm0", BASE + ["--page-size-mode", "0"]),
    ("psm1", BASE + ["--page-size-mode", "1"]),
    ("psm2", BASE + ["--page-size-mode", "2"]),
    ("fit", BASE + ["--fit-page-to-board"]),
    ("classic", BASE + ["-t", "_builtin_classic"]),
    ("unknown_theme", BASE + ["-t", "no_such_theme"]),
    ("subtract", BASE + ["--subtract-soldermask"]),
    ("scale", BASE + ["--scale", "2"]),
    ("bw_negative_mirror", BASE + ["--black-and-white", "--negative", "--mirror",
                                   "--drill-shape-opt", "1"]),
    ("cdnp", BASE + ["--cdnp"]),
    ("sheet", ["--mode-multi", "-l", "F.Cu"]),
]

# qa/tests/cli/test_pcb.py test_pcb_export_svg (the ZoneFill-4.0.7 case; the legacy .brd case
# needs the legacy board reader): `--page-size-mode 1 --exclude-drawing-sheet --black-and-white
# --layers L -o <file>`; `--mode-single` is the default the C++ test relies on, given
# explicitly so that stdout has no deprecation notice.
CLI_TEST_BOARD = "qa/data/cli/artwork_generation_regressions/ZoneFill-4.0.7.kicad_pcb"
CLI_TEST_SETS = [
    ("cli_test_F.Cu", ["--mode-single", "--page-size-mode", "1", "--exclude-drawing-sheet",
                       "--black-and-white", "--layers", "F.Cu"]),
    ("cli_test_B.Cu", ["--mode-single", "--page-size-mode", "1", "--exclude-drawing-sheet",
                       "--black-and-white", "--layers", "B.Cu"]),
]


def normalise(text):
    return re.sub(r"^(<title>SVG Image created as .* date ).*( </title>)$",
                  r"\1<normalised>\2", text, flags=re.M)


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
    out = out_dir
    if "--mode-single" in args:
        out = out_dir + os.path.splitext(os.path.basename(board))[0] + ".svg"
    res = subprocess.run([CLI, "pcb", "export", "svg", *args, "-o", out, inp],
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
    boards.sort()
    for d in EXTRA_DIRS:
        boards.extend(os.path.join(d, f) for f in sorted(os.listdir(os.path.join(KICAD, d)))
                      if f.endswith(".kicad_pcb"))
    runs = [(rel, "default") for rel in boards]
    runs += [(rel, name) for rel in OPTION_BOARDS for name, _ in SETS if name != "default"]
    runs += [(CLI_TEST_BOARD, name) for name, _ in CLI_TEST_SETS]
    sets = dict(SETS + CLI_TEST_SETS)
    lines = []
    for rel, set_name in runs:
        code, out_hash, hashes = run(os.path.join(KICAD, rel), sets[set_name],
                                     os.path.join(SCRATCH, "w"))
        lines.append("B %s %d %s %s\n" % (set_name, code, out_hash, rel))
        lines.extend("%s\n" % h for h in hashes)
    with open(os.path.join(HERE, "corpus.txt"), "w", encoding="utf-8", newline="") as fh:
        fh.write("".join(lines))


if __name__ == "__main__":
    main()
