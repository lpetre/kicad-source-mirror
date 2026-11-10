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

"""STAB-DRC: the text dimension violations of the reference `kicad-cli pcb drc` on the crafted
outline-font board of FONT-RC-1 (`kicad-pcb/tests/data/font_rc1/frc.kicad_pcb`, fonts installed
on the oracle machine) with its rule file at text_thickness min 0.1 / 0.15 / 0.2 / 0.25 mm.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_stab_drc_frc.py > \
        rust/crates/kicad-drc/tests/golden/stab_drc_frc.txt

Output: one line per violation of type text_height / text_thickness, in report order:
`threshold TAB severity TAB type TAB description (TAB uuid TAB description TAB x TAB y)...`,
with newlines in texts written as `\\n`.
"""

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
DATA = HERE.parents[2] / "kicad-pcb" / "tests" / "data" / "font_rc1"
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
THRESHOLDS = ["0.1", "0.15", "0.2", "0.25"]


def esc(s):
    return s.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t")


def num(v):
    return repr(int(v)) if float(v).is_integer() else repr(float(v))


def main():
    rules = (DATA / "frc.kicad_dru").read_text()
    assert "(min 0.25mm)" in rules

    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        env = dict(os.environ, KICAD_ORACLE_HOME=str(tmp / "home"))

        for t in THRESHOLDS:
            d = tmp / t
            d.mkdir()
            shutil.copy(DATA / "frc.kicad_pcb", d / "frc.kicad_pcb")
            (d / "frc.kicad_dru").write_text(rules.replace("(min 0.25mm)", f"(min {t}mm)"))
            subprocess.run([CLI, "pcb", "drc", "--format", "json", "--severity-all", "--units",
                            "mm", "-o", str(d / "out.json"), str(d / "frc.kicad_pcb")],
                           env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            doc = json.loads((d / "out.json").read_text())

            for v in doc["violations"]:
                if v["type"] not in ("text_height", "text_thickness"):
                    continue

                fields = [t, v["severity"], v["type"], esc(v["description"])]

                for i in v["items"]:
                    fields += [i["uuid"], esc(i["description"]), num(i["pos"]["x"]),
                               num(i["pos"]["y"])]

                sys.stdout.write("\t".join(fields) + "\n")


if __name__ == "__main__":
    main()
