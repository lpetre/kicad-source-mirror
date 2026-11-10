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

"""DRC-30 golden: the diff pair coupling entries of the reference kicad-cli DRC report for
every (board, rule variant) case:

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc30_golden.py

A case copies the board (with its .kicad_pro and .kicad_dru) to rust/target/drc30/<case>/,
appends tests/data/drc30/<variant>.dru to the rule file (variant `base` appends nothing) and
runs `kicad-cli pcb drc --format json --severity-all --units mm`. Writes
tests/golden/drc30/reference.txt, one line per entry in report order (tab separated):

    <case> TAB <severity> TAB <type> TAB <description> { TAB <uuid> TAB <item description> TAB <x> TAB <y> }

and `<case> TAB none` for a case without such entries. The Rust test
(tests/drc30_diff_pair_coupling.rs) builds the same rule files.
"""

import json
import os
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
DATA = os.path.join(HERE, "..", "data", "drc30")
SCRATCH = os.path.join(KICAD, "rust", "target", "drc30")
OUT = os.path.join(HERE, "..", "golden", "drc30", "reference.txt")
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
TYPES = ("diff_pair_gap_out_of_range", "diff_pair_uncoupled_length_too_long")

BOARDS = [
    "qa/data/pcbnew/issue7975.kicad_pcb",
    "qa/data/pcbnew/issue8407.kicad_pcb",
    "qa/data/pcbnew/issue17967/issue17967.kicad_pcb",
    "qa/data/pcbnew/diff_pair_uncoupled_tuning_drc.kicad_pcb",
    "qa/data/pcbnew/issue24772/fw16_MCIO8i.kicad_pcb",
    "qa/data/pcbnew/skew_within_diff_pairs_drc.kicad_pcb",
]
VARIANTS = ["base", "gap", "unc", "both", "layer"]
# The large board keeps the golden small with two cases: its own rules, and the variant whose
# keys mix an empty and a set gap rule name.
BOARD_VARIANTS = {"qa/data/pcbnew/issue24772/fw16_MCIO8i.kicad_pcb": ["base", "layer"]}


def run_case(rel, variant):
    case = f"{rel}#{variant}"
    src = os.path.join(KICAD, rel)
    stem = os.path.splitext(os.path.basename(rel))[0]
    work = os.path.join(SCRATCH, os.path.basename(os.path.dirname(rel)) + "_" + stem, variant)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)

    for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
        p = os.path.splitext(src)[0] + ext
        if os.path.exists(p):
            shutil.copy(p, os.path.join(work, stem + ext))

    dru = os.path.join(work, stem + ".kicad_dru")

    if variant != "base":
        text = open(dru).read() if os.path.exists(dru) else "(version 1)\n"
        text += "\n" + open(os.path.join(DATA, variant + ".dru")).read()
        open(dru, "w").write(text)

    out = os.path.join(work, "report.json")
    env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(SCRATCH, "home"))
    subprocess.run(
        [CLI, "pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "-o", out,
         os.path.join(work, stem + ".kicad_pcb")],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    doc = json.load(open(out))
    entries = []

    for v in doc.get("violations", []):
        if v["type"] not in TYPES:
            continue

        severity = v["severity"] + ("/excluded" if v.get("excluded") else "")
        fields = [case, severity, v["type"], v["description"]]

        for it in v["items"]:
            fields += [it["uuid"], it["description"], repr(it["pos"]["x"]),
                       repr(it["pos"]["y"])]

        entries.append("\t".join(x.replace("\t", " ").replace("\n", " ") for x in fields))

    print(f"{case}: {len(entries)}")
    return entries if entries else [case + "\tnone"]


lines = []

for rel in BOARDS:
    for variant in BOARD_VARIANTS.get(rel, VARIANTS):
        lines += run_case(rel, variant)

os.makedirs(os.path.dirname(OUT), exist_ok=True)

with open(OUT, "w") as out:
    out.write("\n".join(lines) + "\n")

print(f"{len(lines)} lines -> {OUT}")
