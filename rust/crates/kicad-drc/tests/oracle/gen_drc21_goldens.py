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

"""Regenerate the expression binding golden of kicad-drc (package DRC-21).

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc21_goldens.py [PCB_DUMP]

Runs `pcb-dump rules` (the real DRC_RULES_PARSER, which compiles every condition with the real
PCBEXPR_UCODE and reports codegen errors with text and offset) on copies of
tests/data/rules/drc21/*.kicad_dru and writes the full dump lines (both modes, rule fields and
condition usage flags included) to tests/golden/drc21_conditions.txt. Replayed by
tests/drc21_pcbexpr.rs with the Rust binding.
"""

import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("gen_rules_goldens",
                                              os.path.join(HERE, "gen_rules_goldens.py"))
rules = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rules)


def main():
    pcb_dump = sys.argv[1] if len(sys.argv) > 1 else "/home/user/kicad-oracle/bin/pcb-dump"
    d = os.path.join(rules.DATA, "drc21")
    files = sorted(f for f in os.listdir(d) if f.endswith(".kicad_dru"))
    dumps = rules.run_dump(pcb_dump, [os.path.join(d, f) for f in files])
    with open(os.path.join(rules.GOLDEN, "drc21_conditions.txt"), "w") as out:
        for f, lines in zip(files, dumps):
            out.write("file|%s|\n" % f)
            for l in lines:
                out.write(l + "\n")
    print("files: %d" % len(files))


if __name__ == "__main__":
    main()
