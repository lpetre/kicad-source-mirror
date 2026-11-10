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
"""DRC-08b: synthetic boards for the copper clearance graphic / zone / teardrop phases.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc08b_boards.py

Writes boards under tests/data/drc08b/:

* `graphics.kicad_pcb` (and `graphics_rules` with a rule file): copper graphics of different
  nets crossing and close to each other, to pads and to zone fills (testGraphicClearances,
  testCopperGraphic filter: shapes of one footprint, same net, UUID dedup), knockout board
  text and knockout footprint fields over one and two zones (testKnockoutTextAgainstZone:
  inherited net, shorts, clearance), a hidden knockout field, plain copper text, text boxes,
  zone pairs on one and two layers (testZonesToZones: gap, overlap, same net, no net) and
  teardrop zones against teardrops and zones (testTeardropClearances).

Connectivity gives every copper cluster without pads a single net (drc08a notes), so the
graphics that must keep different nets start at pads of footprints carrying those nets.

The reference reports go to tests/golden/drc08b/synthetic.txt in the format of
gen_drc08a_corpus.py (default run).
"""

import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gen_drc_corpus as corpus  # noqa: E402
import gen_drc08a_corpus as c08a  # noqa: E402

DATA = os.path.join(HERE, "..", "data", "drc08b")
GOLDEN = os.path.join(HERE, "..", "golden", "drc08b", "synthetic.txt")
CLI = "/home/user/kicad-oracle/bin/kicad-cli"

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
\t\t(35 "F.Fab" user)
\t)
"""

uuid_counter = [0]


def uuid():
    uuid_counter[0] += 1
    return f"d8b00000-0000-4000-8000-{uuid_counter[0]:012d}"


def stroke(width):
    return f"(stroke (width {width}) (type solid))"


def net(name):
    return f' (net "{name}")' if name is not None else ""


def gr_line(x1, y1, x2, y2, layer, netname=None, width=0.2):
    return (f'\t(gr_line (start {x1} {y1}) (end {x2} {y2}) {stroke(width)} (layer "{layer}")'
            f'{net(netname)} (uuid "{uuid()}"))\n')


def gr_rect(x1, y1, x2, y2, layer, netname=None, width=0.2, fill="no"):
    return (f'\t(gr_rect (start {x1} {y1}) (end {x2} {y2}) {stroke(width)} (fill {fill}) '
            f'(layer "{layer}"){net(netname)} (uuid "{uuid()}"))\n')


def gr_circle(x, y, r, layer, netname=None, width=0.2):
    return (f'\t(gr_circle (center {x} {y}) (end {x + r} {y}) {stroke(width)} (fill no) '
            f'(layer "{layer}"){net(netname)} (uuid "{uuid()}"))\n')


def gr_arc(x1, y1, xm, ym, x2, y2, layer, netname=None, width=0.2):
    return (f'\t(gr_arc (start {x1} {y1}) (mid {xm} {ym}) (end {x2} {y2}) {stroke(width)} '
            f'(layer "{layer}"){net(netname)} (uuid "{uuid()}"))\n')


def gr_text(text, x, y, layer, knockout=False, size=1.5, thickness=0.3):
    ko = " knockout" if knockout else ""
    return (f'\t(gr_text "{text}" (at {x} {y} 0) (layer "{layer}"{ko}) (uuid "{uuid()}")\n'
            f'\t\t(effects (font (size {size} {size}) (thickness {thickness})) '
            f'(justify left bottom)))\n')


def gr_text_box(text, x1, y1, x2, y2, layer):
    return (f'\t(gr_text_box "{text}" (start {x1} {y1}) (end {x2} {y2}) (margins 0.5 0.5 0.5 0.5)'
            f' (layer "{layer}") (uuid "{uuid()}")\n'
            f'\t\t(effects (font (size 1 1) (thickness 0.15)) (justify left top))\n'
            f'\t\t(border yes) {stroke(0.2)})\n')


def prop(name, value, x, y, layer, knockout=False, hide=False, size=1.5):
    ko = " knockout" if knockout else ""
    hidden = " (hide yes)" if hide else ""
    return (f'\t\t(property "{name}" "{value}" (at {x} {y} 0) (layer "{layer}"{ko}){hidden}'
            f' (uuid "{uuid()}")\n'
            f'\t\t\t(effects (font (size {size} {size}) (thickness 0.3))))\n')


def footprint(ref, x, y, body, ref_layer="F.Fab", knockout=False, hide=False, ref_at=(0, -3)):
    return (f'\t(footprint "T:{ref}" (layer "F.Cu") (uuid "{uuid()}") (at {x} {y})\n'
            + prop("Reference", ref, ref_at[0], ref_at[1], ref_layer, knockout, hide)
            + prop("Value", "V", 0, 4, "F.Fab", size=1)
            + body + "\t)\n")


def fp_line(x1, y1, x2, y2, layer, width=0.3):
    return (f'\t\t(fp_line (start {x1} {y1}) (end {x2} {y2}) {stroke(width)} (layer "{layer}")'
            f' (uuid "{uuid()}"))\n')


def pad(num, x, y, netname, size=1.0):
    return (f'\t\t(pad "{num}" smd rect (at {x} {y}) (size {size} {size}) (layers "F.Cu")'
            f' (net "{netname}") (uuid "{uuid()}"))\n')


def rect_pts(x1, y1, x2, y2):
    return f"(xy {x1} {y1}) (xy {x2} {y1}) (xy {x2} {y2}) (xy {x1} {y2})"


def zone(netname, layers, pts, teardrop=False, priority=0):
    """A zone with the outline `pts` stored as its fill on every layer of `layers`."""
    lay = " ".join(f'"{l}"' for l in layers)
    attr = "\t\t(attr (teardrop (type padvia)))\n" if teardrop else ""
    fills = "".join(f"\t\t(filled_polygon (layer \"{l}\") (pts {pts}))\n" for l in layers)
    return (f'\t(zone (net "{netname}") (layers {lay}) (uuid "{uuid()}") (hatch edge 0.5)\n'
            f"\t\t(priority {priority})\n{attr}"
            f"\t\t(connect_pads (clearance 0.5)) (min_thickness 0.25)\n"
            f"\t\t(fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1))\n"
            f"\t\t(polygon (pts {pts}))\n{fills}\t)\n")


def outline():
    return gr_rect(90, 40, 240, 140, "Edge.Cuts", width=0.05)


def graphics_board():
    out = HEADER + outline()

    # Graphics against graphics: crossing lines of different nets (short), a line close to a
    # rectangle (clearance), same-net lines crossing (waived), lines without net crossing, an
    # arc near a circle, graphics on an inner layer.
    out += footprint("U3", 100, 45, pad("1", 0, 5, "A") + pad("2", 5, 0, "B")
                     + pad("3", 0, 12, "A") + pad("4", 5, 11, "B"))
    out += gr_line(100, 50, 110, 50, "F.Cu", "A")
    out += gr_line(105, 45, 105, 55, "F.Cu", "B")
    out += gr_rect(100, 52.3, 110, 54, "F.Cu", "C")
    out += gr_line(100, 57, 110, 57, "F.Cu", "A")
    out += gr_line(105, 56, 105, 58, "F.Cu", "A")
    out += gr_line(100, 57.4, 110, 57.4, "F.Cu", "A")
    out += gr_line(105, 56.2, 110, 57.6, "F.Cu", "B")
    out += gr_line(100, 60, 110, 60, "F.Cu")
    out += gr_line(105, 59, 105, 61, "F.Cu")
    out += gr_arc(112, 50, 115, 47, 118, 50, "B.Cu", "D")
    out += gr_circle(115, 51, 0.8, "B.Cu", "E")
    out += gr_line(112, 57, 118, 57, "In1.Cu", "A")
    out += gr_rect(112, 57.25, 118, 59, "In1.Cu", "B", fill="yes")

    # Graphics against pads and tracks: a line of net C through a pad of net A (short), a
    # rectangle without net 0.1 mm from a pad of net B, footprint copper lines crossing each
    # other (same footprint: filtered) and touching a pad of another footprint.
    out += footprint("U1", 130, 50,
                     pad("1", 0, 0, "A") + pad("2", 3, 0, "B")
                     + fp_line(-2, 3, 5, 3, "F.Cu") + fp_line(1.5, 2, 1.5, 4, "F.Cu"))
    out += gr_line(128, 50, 131, 50, "F.Cu", "C")
    out += gr_rect(133.6, 49, 135, 51, "F.Cu")
    out += footprint("U2", 130, 58.6, pad("1", 0, 0, "D"))
    out += f'\t(segment (start 125 62) (end 135 62) (width 0.2) (layer "F.Cu") (net "A") (uuid "{uuid()}"))\n'
    out += gr_line(125, 62.3, 135, 62.3, "F.Cu", "B")

    # Graphics against zones: a circle of net A inside a zone of net B (clearance), a plain
    # copper text over a zone (clearance), a text box over a zone and near a pad.
    out += zone("B", ["F.Cu"], rect_pts(145, 45, 160, 60))
    out += gr_circle(150, 50, 1.5, "F.Cu", "A")
    out += gr_text("TXT", 152, 55, "F.Cu")
    out += gr_text_box("BOX", 156, 56, 163, 59, "F.Cu")
    out += gr_line(144, 62, 162, 62, "F.Cu", "B")
    out += gr_line(144, 44.9, 162, 44.9, "F.Cu", "C")

    # Knockout board text: over two adjacent zones of different nets (inherits the first net,
    # short with the second), over one zone only (nothing), over one zone and 0.1 mm off a
    # zone of another net (clearance), over zones of one net (nothing).
    out += zone("G", ["B.Cu"], rect_pts(100, 70, 110, 80))
    out += zone("H", ["B.Cu"], rect_pts(110.5, 70, 120, 80))
    out += gr_text("KNOCKOUT", 105, 76, "B.Cu", knockout=True)
    out += zone("G", ["In2.Cu"], rect_pts(100, 85, 110, 95))
    out += gr_text("ONE", 102, 91, "In2.Cu", knockout=True)
    out += zone("H", ["In2.Cu"], rect_pts(110.9, 85, 120, 95))
    out += gr_text("NEAR", 107, 94, "In2.Cu", knockout=True, size=1.0, thickness=0.2)
    out += zone("G", ["In1.Cu"], rect_pts(100, 100, 110, 110))
    out += zone("G", ["In1.Cu"], rect_pts(110.5, 100, 120, 110))
    out += gr_text("SAME", 106, 106, "In1.Cu", knockout=True)

    # Knockout footprint fields: a visible reference over two zones of different nets
    # (short), a hidden one (no copper: nothing).
    out += zone("J", ["F.Cu"], rect_pts(125, 70, 135, 80))
    out += zone("K", ["F.Cu"], rect_pts(135.5, 70, 145, 80))
    out += footprint("R1", 135, 75, "", ref_layer="F.Cu", knockout=True, ref_at=(0, 0))
    out += zone("J", ["F.Cu"], rect_pts(125, 85, 135, 95))
    out += zone("K", ["F.Cu"], rect_pts(135.5, 85, 145, 95))
    out += footprint("R2", 135, 90, "", ref_layer="F.Cu", knockout=True, hide=True,
                     ref_at=(0, 0))

    # Zones against zones: a 0.1 mm gap (clearance), an overlap (actual 0), same net
    # overlapping (waived), no-net zones overlapping (waived: both net code 0), a no-net zone
    # against a zone of net M, a two-layer zone against zones on each of its layers.
    out += zone("L", ["F.Cu"], rect_pts(160, 70, 170, 80))
    out += zone("M", ["F.Cu"], rect_pts(170.1, 70, 180, 80))
    out += zone("N", ["F.Cu"], rect_pts(175, 75, 185, 85))
    out += zone("N", ["F.Cu"], rect_pts(182, 82, 190, 90))
    out += zone("", ["B.Cu"], rect_pts(160, 85, 170, 95))
    out += zone("", ["B.Cu"], rect_pts(165, 90, 175, 100))
    out += zone("M", ["B.Cu"], rect_pts(175.15, 85, 180, 95))
    out += zone("P", ["F.Cu", "B.Cu"], rect_pts(190, 70, 200, 80))
    out += zone("Q", ["F.Cu"], rect_pts(200.05, 70, 210, 75))
    out += zone("R", ["B.Cu"], rect_pts(199, 76, 210, 80))
    out += zone("S", ["In1.Cu"], rect_pts(190, 70, 200, 80))

    # Teardrops: two teardrops of different nets overlapping (one test per pair), a teardrop
    # close to a zone of another net, a teardrop inside a zone of its own net (waived), a
    # teardrop against a zone pair (zone-to-zone excludes teardrops).
    out += zone("T", ["F.Cu"], "(xy 160 110) (xy 165 109) (xy 165 111)", teardrop=True,
                priority=30000)
    out += zone("U", ["F.Cu"], "(xy 164 110) (xy 169 109) (xy 169 111)", teardrop=True,
                priority=30000)
    out += zone("V", ["B.Cu"], "(xy 160 120) (xy 165 119) (xy 165 121)", teardrop=True,
                priority=30000)
    out += zone("W", ["B.Cu"], rect_pts(165.1, 115, 175, 125))
    out += zone("X", ["F.Cu"], rect_pts(175, 105, 185, 115))
    out += zone("X", ["F.Cu"], "(xy 178 110) (xy 183 109) (xy 183 111)", teardrop=True,
                priority=30000)
    out += zone("Y", ["F.Cu"], "(xy 184 110) (xy 189 109) (xy 189 111)", teardrop=True,
                priority=30000)

    out += "\t(embedded_fonts no)\n)\n"
    return out


# A rule file: 0.5 mm clearance between zones of nets L and M (condition without function
# calls, evaluated by the DRC-21 binding) and 0.3 mm for everything else.
RULES = """(version 1)
(rule "zones LM"
\t(constraint clearance (min 0.5mm))
\t(condition "A.NetName == 'L' || B.NetName == 'L'"))
(rule "all"
\t(constraint clearance (min 0.3mm)))
"""


def main():
    os.makedirs(DATA, exist_ok=True)
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="drc08b-home-"))
    text = graphics_board()

    for name, rules in (("graphics", None), ("graphics_rules", RULES)):
        with open(os.path.join(DATA, name + ".kicad_pcb"), "w") as f:
            f.write(text)

        dru = os.path.join(DATA, name + ".kicad_dru")

        if rules:
            with open(dru, "w") as f:
                f.write(rules)
        elif os.path.exists(dru):
            os.remove(dru)

    lines = []

    for name in ("graphics", "graphics_rules"):
        rel = os.path.relpath(os.path.join(DATA, name + ".kicad_pcb"), corpus.KICAD)
        lines += c08a.lines_for(rel, corpus.run_board(CLI, rel, env))

    c08a.write(GOLDEN, lines)


if __name__ == "__main__":
    main()
