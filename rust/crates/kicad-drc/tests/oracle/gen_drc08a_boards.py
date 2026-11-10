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
"""DRC-08a: synthetic boards for the copper clearance item tests and their reference reports.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc08a_boards.py

Few corpus boards the port loads have copper clearance violations, so this writes boards
under tests/data/drc08a/ with tracks, arcs, vias (through, blind, micro, unused layers
removed), zones with stored fills and nets placed to hit every branch of
testSingleLayerItemAgainstItem and testItemAgainstZone (crossing tracks, clearance, shorts,
`<no net>` shorts, hole clearance against unflashed vias, hole clearance 0, zone clearance and
zone hole clearance, same-net waivers, several violations on one track for
`--all-track-errors`). The reference reports (default and `--all-track-errors`) go to
tests/golden/drc08a/synthetic.txt and synthetic_ate.txt in the format of
gen_drc08a_corpus.py.
"""

import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gen_drc_corpus as corpus  # noqa: E402
import gen_drc08a_corpus as c08a  # noqa: E402

DATA = os.path.join(HERE, "..", "data", "drc08a")

HEADER = """(kicad_pcb
\t(version 20260819)
\t(generator "pcbnew")
\t(generator_version "10.99")
\t(general
\t\t(thickness 1.6)
\t\t(legacy_teardrops no)
\t)
\t(paper "A4")
\t(layers
\t\t(0 "F.Cu" signal)
\t\t(4 "In1.Cu" signal)
\t\t(6 "In2.Cu" signal)
\t\t(2 "B.Cu" signal)
\t\t(25 "Edge.Cuts" user)
\t)
"""

uuid_counter = [0]


def uuid():
    uuid_counter[0] += 1
    return f"d8a00000-0000-4000-8000-{uuid_counter[0]:012d}"


def segment(x1, y1, x2, y2, layer, net, width=0.2):
    return f"""\t(segment
\t\t(start {x1} {y1})
\t\t(end {x2} {y2})
\t\t(width {width})
\t\t(layer "{layer}")
\t\t(net "{net}")
\t\t(uuid "{uuid()}")
\t)
"""


def arc(x1, y1, xm, ym, x2, y2, layer, net, width=0.2):
    return f"""\t(arc
\t\t(start {x1} {y1})
\t\t(mid {xm} {ym})
\t\t(end {x2} {y2})
\t\t(width {width})
\t\t(layer "{layer}")
\t\t(net "{net}")
\t\t(uuid "{uuid()}")
\t)
"""


def via(x, y, net, layers=("F.Cu", "B.Cu"), size=0.6, drill=0.3, extra=""):
    lay = " ".join(f'"{l}"' for l in layers)
    return f"""\t(via{extra}
\t\t(at {x} {y})
\t\t(size {size})
\t\t(drill {drill})
\t\t(layers {lay})
\t\t(net "{net}")
\t\t(uuid "{uuid()}")
\t)
"""


def via_unused_removed(x, y, net):
    return f"""\t(via
\t\t(at {x} {y})
\t\t(size 0.8)
\t\t(drill 0.4)
\t\t(layers "F.Cu" "B.Cu")
\t\t(remove_unused_layers yes)
\t\t(keep_end_layers yes)
\t\t(net "{net}")
\t\t(uuid "{uuid()}")
\t)
"""


def zone(net, layer, x1, y1, x2, y2):
    pts = f"(xy {x1} {y1}) (xy {x2} {y1}) (xy {x2} {y2}) (xy {x1} {y2})"
    return f"""\t(zone
\t\t(net "{net}")
\t\t(layer "{layer}")
\t\t(uuid "{uuid()}")
\t\t(hatch edge 0.5)
\t\t(connect_pads
\t\t\t(clearance 0.5)
\t\t)
\t\t(min_thickness 0.25)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode 1)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{pts}
\t\t\t)
\t\t)
\t\t(filled_polygon
\t\t\t(layer "{layer}")
\t\t\t(pts
\t\t\t\t{pts}
\t\t\t)
\t\t)
\t)
"""


def outline():
    return f"""\t(gr_rect
\t\t(start 90 50)
\t\t(end 220 120)
\t\t(stroke
\t\t\t(width 0.05)
\t\t\t(type default)
\t\t)
\t\t(fill no)
\t\t(layer "Edge.Cuts")
\t\t(uuid "{uuid()}")
\t)
"""


def items_board():
    out = HEADER + outline()

    # Crossing tracks (tracks_crossing), and a T junction (touching end, also crossing).
    out += segment(100, 60, 110, 60, "F.Cu", "A")
    out += segment(105, 55, 105, 65, "F.Cu", "B")
    out += segment(100, 63, 104.9, 63, "B.Cu", "C")
    out += segment(105, 61, 105, 66, "B.Cu", "A")

    # Parallel tracks 0.1 mm apart (clearance), and same-net tracks 0 mm apart (waived).
    out += segment(100, 70, 110, 70, "F.Cu", "A")
    out += segment(100, 70.3, 110, 70.3, "F.Cu", "B")
    out += segment(100, 72, 110, 72, "F.Cu", "A")
    out += segment(100, 72.1, 110, 72.1, "F.Cu", "A")

    # A track with no net overlapping one of net B: actual 0 -> shorting_items with <no net>
    # (overlap without a segment intersection: collinear tracks side by side).
    out += segment(100, 75, 110, 75, "B.Cu", "B")
    out += segment(102, 75.15, 108, 75.15, "B.Cu", "")

    # A via of net B on a track of net A (short), a via 0.1 mm off a track (clearance),
    # two vias of different nets close together (clearance, then the hole test).
    out += segment(120, 60, 130, 60, "F.Cu", "A")
    out += via(125, 60.2, "B")
    out += segment(120, 65, 130, 65, "B.Cu", "A")
    out += via(125, 65.5, "C")
    out += via(121, 68, "A")
    out += via(121.7, 68, "B")

    # Arcs: an arc of net A next to a track of net B, an arc of net C shorting a track of A.
    out += arc(120, 75, 125, 72, 130, 75, "F.Cu", "A")
    out += segment(120, 72.2, 130, 72.2, "F.Cu", "B")
    out += arc(120, 80, 125, 78, 130, 80, "B.Cu", "C")
    out += segment(124, 78, 126, 78, "B.Cu", "A")

    # One track of net D violating against several others (one or all reported).
    out += segment(140, 60, 150, 60, "In1.Cu", "D")
    out += segment(141, 60.25, 143, 60.25, "In1.Cu", "A")
    out += segment(144, 59.75, 146, 59.75, "In1.Cu", "B")
    out += segment(147, 58, 147, 62, "In1.Cu", "C")
    out += via(149, 60.4, "C")

    # Unflashed vias: a through via with unused layers removed, a track of another net on an
    # inner layer close to its hole (hole_clearance against the hole shape).
    out += via_unused_removed(160, 60, "A")
    out += segment(155, 60.3, 165, 60.3, "In2.Cu", "B")
    out += via_unused_removed(160, 65, "A")
    out += segment(155, 65.1, 165, 65.1, "In2.Cu", "")

    # Blind via F.Cu - In1.Cu with a track on B.Cu crossing its position (not on the via's
    # layers: no clearance or hole test); a micro via near a track.
    out += via(170, 60, "A", ("F.Cu", "In1.Cu"), extra=" blind")
    out += segment(165, 60, 175, 60, "B.Cu", "B")
    out += via(170, 65, "A", ("F.Cu", "In1.Cu"), size=0.3, drill=0.1, extra=" micro")
    out += segment(165, 65.3, 175, 65.3, "F.Cu", "B")

    # Zones: a GND fill on F.Cu with a track 0.1 mm off its edge, a via 0.1 mm off it, a
    # same-net track crossing it (waived), a no-net zone on B.Cu crossed by a track.
    out += zone("GND", "F.Cu", 180, 60, 200, 80)
    out += segment(185, 59.7, 195, 59.7, "F.Cu", "B")
    out += via(179.5, 70, "C")
    out += segment(190, 55, 190, 85, "F.Cu", "GND")
    out += zone("", "B.Cu", 180, 90, 200, 110)
    out += segment(185, 85, 185, 115, "B.Cu", "A")
    out += via(201, 95, "B", ("F.Cu", "B.Cu"), size=1.5, drill=1.2)

    out += "\t(embedded_fonts no)\n)\n"
    return out


# A rule file: hole clearance 0.4 mm (zone hole test and the via hole tests need it > 0),
# and a 0 hole clearance rule for vias (the "actual < 0" message).
RULES = """(version 1)
(rule "hole clearance"
\t(constraint hole_clearance (min 0.4mm)))
"""


def main():
    os.makedirs(DATA, exist_ok=True)
    boards = {"items": (items_board(), None), "items_rules": (None, RULES)}
    rels = []

    for name, (text, rules) in boards.items():
        pcb = os.path.join(DATA, name + ".kicad_pcb")

        if text is None:
            text = boards["items"][0]

        with open(pcb, "w") as f:
            f.write(text)

        dru = os.path.join(DATA, name + ".kicad_dru")

        if rules:
            with open(dru, "w") as f:
                f.write(rules)
        elif os.path.exists(dru):
            os.remove(dru)

        rels.append(os.path.relpath(pcb, corpus.KICAD))

    cli = "/home/user/kicad-oracle/bin/kicad-cli"
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="drc08a-home-"))
    default, ate = [], []

    for rel in rels:
        default += c08a.lines_for(rel, corpus.run_board(cli, rel, env))
        ate += c08a.lines_for(rel, c08a.run_ate(cli, rel, env))

    c08a.write(os.path.join(c08a.GOLDEN_DIR, "synthetic.txt"), default)
    c08a.write(os.path.join(c08a.GOLDEN_DIR, "synthetic_ate.txt"), ate)


if __name__ == "__main__":
    main()
