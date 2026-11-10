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

"""DRC-07: synthetic boards for the connectivity provider and their reference reports.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc07_boards.py

No corpus board that the port loads has a `track_on_post_machined_layer` or
`track_not_centered_on_via` violation, so this writes tests/data/drc07/post_machined.kicad_pcb
(+ .kicad_pro with `track_not_centered_on_via` raised to error): the 10-layer stackup and
setup of qa/data/pcbnew/issue25021/backdrill.kicad_pcb with backdrilled / tertiary-drilled /
plain vias, tracks on drilled and undrilled layers, off-centre track ends (alone on their
layer, next to a centred track, both ends inside one via), dangling ends and an unconnected
net. The reference runs through gen_drc_corpus.run_board (cached under rust/target/drc-corpus/)
and tests/golden/drc07_synthetic.txt gets the entries of the provider's types, the
`unconnected_items` array included, in the drc_corpus.txt format.
"""

import json
import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import gen_drc_corpus as corpus  # noqa: E402

SRC = os.path.join(corpus.KICAD, "qa", "data", "pcbnew", "issue25021", "backdrill")
DATA = os.path.join(corpus.HERE, "..", "data", "drc07")
GOLDEN = os.path.join(corpus.HERE, "..", "golden", "drc07_synthetic.txt")
TYPES = {"track_dangling", "via_dangling", "track_on_post_machined_layer",
         "track_not_centered_on_via", "isolated_copper", "unconnected_items"}

uuid_counter = [0]


def uuid():
    uuid_counter[0] += 1
    return f"d7000000-0000-4000-8000-{uuid_counter[0]:012d}"


def via(x, y, net, extra=""):
    return f"""\t(via
\t\t(at {x} {y})
\t\t(size 0.6)
\t\t(drill 0.3)
{extra}\t\t(layers "F.Cu" "B.Cu")
\t\t(net "{net}")
\t\t(uuid "{uuid()}")
\t)
"""


def segment(x1, y1, x2, y2, layer, net):
    return f"""\t(segment
\t\t(start {x1} {y1})
\t\t(end {x2} {y2})
\t\t(width 0.2)
\t\t(layer "{layer}")
\t\t(net "{net}")
\t\t(uuid "{uuid()}")
\t)
"""


BACKDRILL_FRONT = """\t\t(backdrill
\t\t\t(size 0.33)
\t\t\t(layers "F.Cu" "In3.Cu")
\t\t)
"""
TERTIARY_BACK = """\t\t(tertiary_drill
\t\t\t(size 0.33)
\t\t\t(layers "B.Cu" "In6.Cu")
\t\t)
"""


def board():
    with open(SRC + ".kicad_pcb") as f:
        lines = f.read().split("\n")

    # Header, stackup, setup and the board outline of the source board (up to its vias).
    first_via = next(i for i, l in enumerate(lines) if l == "\t(via")
    out = "\n".join(lines[:first_via]) + "\n"

    # Backdrilled from the front (F.Cu .. In3.Cu): tracks on F.Cu and In2.Cu are on drilled
    # layers, the In5.Cu track is not.
    out += via(105, 66, "A", BACKDRILL_FRONT)
    out += segment(105, 66, 105, 60, "F.Cu", "A")
    out += segment(105, 66, 100, 66, "In5.Cu", "A")
    out += segment(100, 66, 100, 70, "In5.Cu", "A")
    out += segment(105.1, 66, 105, 72, "In2.Cu", "A")

    # Tertiary drill from the back (B.Cu .. In6.Cu); one off-centre track on an undrilled
    # layer.
    out += via(110, 66, "B", TERTIARY_BACK)
    out += segment(110, 66, 110, 72, "B.Cu", "B")
    out += segment(110.1, 66, 110, 60, "In2.Cu", "B")
    out += segment(110, 65.9, 112, 60, "In7.Cu", "B")

    # Plain via: off-centre next to a centred track on F.Cu (no violation), alone on B.Cu.
    out += via(115, 66, "C")
    out += segment(115.15, 66, 115, 60, "F.Cu", "C")
    out += segment(115, 66, 118, 60, "F.Cu", "C")
    out += segment(115, 66.1, 115, 72, "B.Cu", "C")

    # Both ends of a short track inside one via, off centre.
    out += via(120, 66, "D")
    out += segment(119.9, 66, 120.1, 66, "F.Cu", "D")
    out += segment(120, 66, 120, 72, "B.Cu", "D")

    # An unconnected net: two vias and a dangling track.
    out += via(130, 66, "E")
    out += via(135, 70, "E")
    out += segment(130, 66, 132, 60, "F.Cu", "E")

    out += "\t(embedded_fonts no)\n)\n"
    return out


def main():
    os.makedirs(DATA, exist_ok=True)
    pcb = os.path.join(DATA, "post_machined.kicad_pcb")

    with open(pcb, "w") as f:
        f.write(board())

    with open(SRC + ".kicad_pro") as f:
        pro = json.load(f)

    pro["board"]["design_settings"]["rule_severities"]["track_not_centered_on_via"] = "error"
    pro["meta"]["filename"] = "post_machined.kicad_pro"

    with open(os.path.join(DATA, "post_machined.kicad_pro"), "w") as f:
        json.dump(pro, f, indent=2)
        f.write("\n")

    rel = os.path.relpath(pcb, corpus.KICAD)
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="drc07-home-"))
    report = corpus.run_board("/home/user/kicad-oracle/bin/kicad-cli", rel, env)

    if report is not None:
        report = {"violations": report.get("violations", []) +
                  report.get("unconnected_items", [])}

    corpus.PORTED_TYPES = TYPES
    lines = [l for l in corpus.golden_lines(rel, report)
             if l.split("\t")[1] in ("load_error", "none") or
             (l.split("\t")[1] == "#" and l.split("\t")[2] in TYPES) or
             (l.split("\t")[1] not in ("#",) and l.split("\t")[3] in TYPES)]

    with open(GOLDEN, "w") as f:
        f.write("\n".join(lines) + "\n")

    print(f"{len(lines)} lines -> {GOLDEN}")


if __name__ == "__main__":
    main()
