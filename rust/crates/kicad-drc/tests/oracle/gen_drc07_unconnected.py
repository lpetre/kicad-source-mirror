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

"""DRC-07: the `unconnected_items` part of the reference DRC reports of the shared corpus.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc07_unconnected.py [BOARD...]

`gen_drc_corpus.py` keeps only the `violations` array of
`kicad-cli pcb drc --format json --severity-all --units mm`; the ratsnest markers
(`DRCE_UNCONNECTED_ITEMS`) are written to the separate `unconnected_items` array. This script
takes that array from the same cached reports (rust/target/drc-corpus/, running the reference
where the cache is missing, exactly as gen_drc_corpus.py does) and writes
tests/golden/drc07_unconnected.txt in the format of drc_corpus.txt (entries in report order,
full up to MAX_FULL per board, else one summary line with count and FNV-1a 64).
"""

import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import gen_drc_corpus as corpus  # noqa: E402

GOLDEN = os.path.join(corpus.HERE, "..", "golden", "drc07_unconnected.txt")


def main():
    boards = sys.argv[1:] or sorted(corpus.corpus())
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="drc-corpus-home-"))
    corpus.PORTED_TYPES = {"unconnected_items"}
    lines = {}

    if sys.argv[1:] and os.path.exists(GOLDEN):
        with open(GOLDEN) as f:
            for line in f:
                lines.setdefault(line.split("\t", 1)[0], []).append(line.rstrip("\n"))

    for rel in boards:
        report = corpus.run_board("/home/user/kicad-oracle/bin/kicad-cli", rel, env)

        if report is not None:
            report = {"violations": report.get("unconnected_items", [])}

        lines[rel] = corpus.golden_lines(rel, report)

    with open(GOLDEN, "w") as f:
        for rel in sorted(lines):
            f.write("\n".join(lines[rel]) + "\n")

    print(f"{len(boards)} boards -> {GOLDEN}")


if __name__ == "__main__":
    main()
