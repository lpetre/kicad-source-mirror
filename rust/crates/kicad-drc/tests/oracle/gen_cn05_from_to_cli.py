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
"""CN-05: tests/golden/cn05_from_to_cli.txt, the tracks the reference kicad-cli flags with a
rule `(constraint track_width (min 10mm)) (condition "A.fromTo('<from>','<to>')")` on
demos/ecc83/ecc83-pp (one `pcb drc` run per pair, on a scratch copy of the board).

Line format: `pair|<from>|<to>|<sorted track UUIDs, comma separated>`.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../.."))
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
BOARD = os.path.join(KICAD, "demos/ecc83/ecc83-pp")
OUT = os.path.join(HERE, "../golden/cn05_from_to_cli.txt")

PAIRS = [
    ("U1-1", "U1-7"),
    ("U1-1", "R1-1"),
    ("R4-*", "P1-2"),
    ("P2-1", "R3-1"),
    ("R4-1", "P1"),
    ("U1-1", "*"),
    ("u1-?", "r1-1"),
]


def main():
    lines = []

    with tempfile.TemporaryDirectory() as tmp:
        os.environ["KICAD_ORACLE_HOME"] = os.path.join(tmp, "home")

        for frm, to in PAIRS:
            for ext in ("kicad_pcb", "kicad_pro"):
                shutil.copy(BOARD + "." + ext, os.path.join(tmp, "b." + ext))

            with open(os.path.join(tmp, "b.kicad_dru"), "w") as f:
                f.write("(version 1)\n(rule ft (constraint track_width (min 10mm)) "
                        f"(condition \"A.fromTo('{frm}','{to}')\"))\n")

            out = os.path.join(tmp, "out.json")
            subprocess.run([CLI, "pcb", "drc", "--format", "json", "--severity-all", "-o", out,
                            os.path.join(tmp, "b.kicad_pcb")],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

            with open(out) as f:
                report = json.load(f)

            uuids = sorted(i["uuid"] for v in report["violations"]
                           if v["type"] == "track_width" for i in v["items"])
            lines.append(f"pair|{frm}|{to}|{','.join(uuids)}")

    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")

    print(f"{len(lines)} pairs -> {OUT}", file=sys.stderr)


if __name__ == "__main__":
    main()
