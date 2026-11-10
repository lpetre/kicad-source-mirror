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
"""FAB-13 goldens: drill maps and drill charts plotted by the reference kicad-cli.

    python3 generate.py

Boards: the stage-1 cases of kicad-pcb tests/data/m26/cases.txt (base board from qa/data/pcbnew,
optional drill symbol profile, the drill charts / stack-up tables of tables.txt) with the drill
maps of tests/data/plot_drill_symbols/maps.txt appended (tests/plot_drill_symbols.rs builds the
same text). The boards are not re-saved: the refreshed tables would make them too large to commit,
and plotting them as they are also exercises `RefreshGeneratedTables`, which every kicad-cli plot
runs first (`PCB_PLOTTER::Plot`).

Plots (on copies in a scratch dir):
* `pcb export gerbers -l <LAYERS>`: one file per layer;
* `pcb export svg --mode-single --exclude-drawing-sheet --drill-shape-opt 0 -l <layer>` per
  layer of LAYERS, and `--drill-shape-opt 2 -l Eco1.User` (a map layer: no drill marks).

golden.txt: `<case> <file> <FNV-1a 64 of the normalised file>`; Gerber normalisation as
tests/plot_common (`normalise`), SVG: the `<title>` date cut as in tests/plot_driver.rs. The
normalised outputs stay in $SCRATCH/ref/<case>/ for diagnosis (KICAD_FAB_DRILLSYM_REF).
"""
import os
import re
import shutil
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../plot_driver"))
from generate import normalise, fnv  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../../.."))
CRATE = os.path.abspath(os.path.join(HERE, "../../.."))
M26 = os.path.join(KICAD, "rust/crates/kicad-pcb/tests/data/m26")
QA = os.path.join(KICAD, "qa/data/pcbnew")
DATA = os.path.join(CRATE, "tests/data/plot_drill_symbols")
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
SCRATCH = os.environ.get("SCRATCH", os.path.join(KICAD, "rust/target/fab13-golden"))

LAYERS = ["Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "User.1", "B.Fab"]


def cases():
    out = []
    for line in open(os.path.join(M26, "cases.txt")):
        if line.startswith("#") or not line.strip():
            continue
        name, base, profile, mode = [f.strip() for f in line.split("|")]
        if mode == "tables":
            out.append((len(out), name, base, profile))
    return out


def board_text(index, base, profile):
    """kicad-pcb m26_generated_tables.py `stage1_input`, plus the drill maps."""
    text = open(os.path.join(QA, base)).read().rstrip()
    if profile != "-":
        prof = open(os.path.join(M26, profile)).read().rstrip("\n")
        idx = text.index("(setup") + len("(setup")
        text = text[:idx] + "\n" + prof + text[idx:]
    extra = open(os.path.join(M26, "tables.txt")).read() + open(os.path.join(DATA, "maps.txt")).read()
    for k in range(12):
        extra = extra.replace("@UUID%X@" % k, "%08x-0000-4000-8000-%012d" % (0x13000000 + index, k))
    return text[:-1] + extra + ")\n"


def cli(args):
    env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(SCRATCH, "home"))
    return subprocess.run([CLI, *args], env=env, capture_output=True, text=True, timeout=600)


def strip_svg_date(text):
    out = []
    for line in text.split("\n"):
        i = line.find(" date ")
        if line.startswith("<title>") and i >= 0:
            line = line[:i] + "<date>"
        out.append(line)
    return "\n".join(out)


def main():
    golden = ["# FAB-13: <case> <file> <fnv of the normalised file> (generate.py)"]
    for index, name, base, profile in cases():
        work = os.path.join(SCRATCH, "w")
        shutil.rmtree(work, ignore_errors=True)
        os.makedirs(work)
        board = os.path.join(work, name + ".kicad_pcb")
        open(board, "w").write(board_text(index, base, profile))

        ref = os.path.join(SCRATCH, "ref", name)
        shutil.rmtree(ref, ignore_errors=True)
        os.makedirs(ref)

        res = cli(["pcb", "export", "gerbers", "-l", ",".join(LAYERS), "-o", work + "/out/", board])
        for f in re.findall(r"Plotted to '([^']*)'", res.stdout):
            with open(f, encoding="utf-8", newline="") as fh:
                text = normalise(fh.read())
            fname = os.path.basename(f)
            if fname.endswith(".gbrjob"):
                continue
            golden.append("%s %s %s" % (name, fname, fnv(text)))
            open(os.path.join(ref, fname), "w", newline="").write(text)

        svg_runs = [(layer, "0") for layer in LAYERS] + [("Eco1.User", "2")]
        for layer, drill in svg_runs:
            fname = "%s-%s.svg" % (layer.replace(".", "_"), drill)
            out = os.path.join(work, fname)
            res = cli(["pcb", "export", "svg", "--mode-single", "--exclude-drawing-sheet",
                       "--drill-shape-opt", drill, "-l", layer, "-o", out, board])
            assert res.returncode == 0, res.stdout + res.stderr
            with open(out, encoding="utf-8", newline="") as fh:
                text = strip_svg_date(fh.read())
            golden.append("%s %s %s" % (name, fname, fnv(text)))
            open(os.path.join(ref, fname), "w", newline="").write(text)

        shutil.rmtree(work, ignore_errors=True)

    with open(os.path.join(HERE, "golden.txt"), "w") as fh:
        fh.write("\n".join(golden) + "\n")


if __name__ == "__main__":
    main()
