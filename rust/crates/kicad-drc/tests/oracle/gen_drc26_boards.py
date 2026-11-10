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

"""DRC-26 (silk clearance and physical clearance providers): synthetic boards without footprints
and their reference violations.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc26_boards.py [--cli KICAD_CLI]

Writes tests/data/silk_physical/*.kicad_pcb (+ .kicad_pro / .kicad_dru) and, for each board, runs
the reference on a copy:

    kicad-cli pcb drc --format json --severity-all --units mm

and stores the entries of the two providers in report order as
tests/golden/drc26/<board>.txt, one entry per line:

    <severity> TAB <type> TAB <description> { TAB <uuid> TAB <item description> TAB <x> TAB <y> }

Entries of the providers: `silk_overlap`, the two-item `silk_over_copper` entries (the one-item
ones come from the solder mask provider), and the `clearance` / `hole_clearance` entries whose
description names one of the physical rules of the board (`rule 'phys'`, `rule 'physhole'`) or
starts with "Internal clearance violation" (the other ones come from the copper clearance and
hole clearance providers).
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data", "silk_physical")
GOLDEN = os.path.join(HERE, "..", "golden", "drc26")

HEADER = """(kicad_pcb
\t(version 20260206)
\t(generator "pcbnew")
\t(generator_version "10.0")
\t(general
\t\t(thickness 1.6)
\t\t(legacy_teardrops no)
\t)
\t(paper "A4")
\t(layers
\t\t(0 "F.Cu" signal)
\t\t(2 "B.Cu" signal)
\t\t(9 "F.Adhes" user "F.Adhesive")
\t\t(11 "B.Adhes" user "B.Adhesive")
\t\t(13 "F.Paste" user)
\t\t(15 "B.Paste" user)
\t\t(5 "F.SilkS" user "F.Silkscreen")
\t\t(7 "B.SilkS" user "B.Silkscreen")
\t\t(1 "F.Mask" user)
\t\t(3 "B.Mask" user)
\t\t(25 "Edge.Cuts" user)
\t\t(27 "Margin" user)
\t\t(31 "F.CrtYd" user "F.Courtyard")
\t\t(29 "B.CrtYd" user "B.Courtyard")
\t\t(35 "F.Fab" user)
\t\t(33 "B.Fab" user)
\t)
\t(setup
\t\t(pad_to_mask_clearance 0)
\t\t(solder_mask_min_width {web})
\t\t(tenting
\t\t\t(front yes)
\t\t\t(back yes)
\t\t)
\t)
"""

PRO = """{
  "board": {
    "design_settings": {
      "rules": {
        "min_silk_clearance": %s
      }
    }
  },
  "meta": {
    "filename": "%s.kicad_pro",
    "version": 3
  }
}
"""

_uuid = [0]


def uid():
    _uuid[0] += 1
    return "00000000-0000-0000-0000-%012x" % _uuid[0]


def seg(x1, y1, x2, y2, w, net, layer="F.Cu"):
    return '\t(segment (start %s %s) (end %s %s) (width %s) (layer "%s") (net "%s") (uuid "%s"))\n' % (
        x1, y1, x2, y2, w, layer, net, uid())


def via(x, y, size, drill, net, tented=True):
    t = "" if tented else " (tenting (front no) (back no))"
    return '\t(via (at %s %s) (size %s) (drill %s) (layers "F.Cu" "B.Cu")%s (net "%s") (uuid "%s"))\n' % (
        x, y, size, drill, t, net, uid())


def rect(x1, y1, x2, y2, layer, fill=True, width=0.05):
    return ('\t(gr_rect (start %s %s) (end %s %s) (stroke (width %s) (type solid)) (fill %s) '
            '(layer "%s") (uuid "%s"))\n') % (x1, y1, x2, y2, width, "yes" if fill else "no", layer, uid())


def circle(cx, cy, r, layer, fill=False, width=0.12):
    return ('\t(gr_circle (center %s %s) (end %s %s) (stroke (width %s) (type solid)) (fill %s) '
            '(layer "%s") (uuid "%s"))\n') % (cx, cy, cx + r, cy, width, "yes" if fill else "no", layer, uid())


def line(x1, y1, x2, y2, layer, width=0.15):
    return ('\t(gr_line (start %s %s) (end %s %s) (stroke (width %s) (type solid)) (layer "%s") '
            '(uuid "%s"))\n') % (x1, y1, x2, y2, width, layer, uid())


def garc(s, m, e, layer, width=0.15):
    return ('\t(gr_arc (start %s %s) (mid %s %s) (end %s %s) (stroke (width %s) (type solid)) '
            '(layer "%s") (uuid "%s"))\n') % (s[0], s[1], m[0], m[1], e[0], e[1], width, layer, uid())


def curve(pts, layer, width=0.15):
    p = " ".join("(xy %s %s)" % xy for xy in pts)
    return ('\t(gr_curve (pts %s) (stroke (width %s) (type solid)) (layer "%s") (uuid "%s"))\n') % (
        p, width, layer, uid())


def poly(pts, layer, width=0.1, fill=True):
    p = " ".join("(xy %s %s)" % xy for xy in pts)
    return ('\t(gr_poly (pts %s) (stroke (width %s) (type solid)) (fill %s) (layer "%s") '
            '(uuid "%s"))\n') % (p, width, "yes" if fill else "no", layer, uid())


def text(s, x, y, layer, size=1.0, thickness=0.15, mirror=False, hide=False):
    j = " (justify mirror)" if mirror else ""
    return ('\t(gr_text "%s" (at %s %s 0) (layer "%s") (uuid "%s") (effects (font (size %s %s) '
            '(thickness %s))%s))\n') % (s, x, y, layer, uid(), size, size, thickness, j)


def zone(net, layer, outline, fills, rule_area=False):
    pts = " ".join("(xy %s %s)" % xy for xy in outline)
    keep = ""

    if rule_area:
        keep = """\t\t(keepout
\t\t\t(tracks not_allowed)
\t\t\t(vias not_allowed)
\t\t\t(pads not_allowed)
\t\t\t(copperpour allowed)
\t\t\t(footprints allowed)
\t\t)
"""

    filled = ""

    for f in fills:
        fp = " ".join("(xy %s %s)" % xy for xy in f)
        filled += """\t\t(filled_polygon
\t\t\t(layer "%s")
\t\t\t(pts
\t\t\t\t%s
\t\t\t)
\t\t)
""" % (layer, fp)

    return """\t(zone
\t\t(net "%s")
\t\t(layer "%s")
\t\t(uuid "%s")
\t\t(hatch edge 0.5)
\t\t(connect_pads
\t\t\t(clearance 0.25)
\t\t)
\t\t(min_thickness 0.25)
%s\t\t(fill%s
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode 0)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t%s
\t\t\t)
\t\t)
%s\t)
""" % (net, layer, uid(), keep, "" if rule_area else " yes", pts, filled)


def square(x1, y1, x2, y2):
    return [(x1, y1), (x2, y1), (x2, y2), (x1, y2)]


def silk_items():
    out = []
    # 1. Silk lines overlapping each other (front, back), a silk text over a silk line.
    out.append(line(100, 100, 110, 100, "F.SilkS"))
    out.append(line(105, 98, 105, 102, "F.SilkS"))
    out.append(line(100, 100.3, 110, 100.3, "F.SilkS"))
    out.append(text("SILK", 104, 101.2, "F.SilkS", 1.0, 0.15))
    out.append(line(100, 104, 110, 104, "B.SilkS"))
    out.append(line(105, 102, 105, 106, "B.SilkS"))
    out.append(text("BK", 108, 104.5, "B.SilkS", 1.0, 0.15, mirror=True))
    # 2. Silk over copper tracks, tented and untented vias, a mask opening.
    out.append(line(112, 98, 112, 110, "F.SilkS"))
    out.append(seg(110, 100, 116, 100, 0.25, "A"))
    out.append(seg(110, 101.5, 116, 101.5, 0.25, "B"))
    out.append(via(112.4, 104, 0.8, 0.4, "A", tented=True))
    out.append(via(112.4, 106, 0.8, 0.4, "B", tented=False))
    out.append(rect(111, 108, 114, 109, "F.Mask"))
    out.append(line(115, 108.5, 118, 108.5, "B.SilkS"))
    out.append(rect(116, 108, 117, 109, "B.Mask"))
    # 3. Silk against courtyard, fab, paste, adhesive graphics and Margin.
    out.append(line(120, 98, 120, 110, "F.SilkS"))
    out.append(line(119, 100, 121, 100, "F.CrtYd", 0.05))
    out.append(line(119, 102, 121, 102, "F.Fab", 0.1))
    out.append(rect(119.5, 104, 120.5, 104.5, "F.Paste"))
    out.append(circle(120, 106, 0.3, "F.Adhes", fill=True, width=0.05))
    out.append(line(124, 98, 124, 110, "B.SilkS"))
    out.append(line(123, 100, 125, 100, "B.CrtYd", 0.05))
    out.append(line(123, 102, 125, 102, "B.Fab", 0.1))
    out.append(line(118, 111, 126, 111, "Margin", 0.05))
    out.append(line(122, 110.8, 122, 111.5, "F.SilkS"))
    # 4. Compound graphics: board-level shapes against each other are never reported.
    out.append(rect(127, 99, 130, 102, "F.SilkS", fill=False, width=0.12))
    out.append(circle(128.5, 100.5, 1.5, "F.SilkS"))
    # 5. Silk across the board edge and silk outside the board (skipped).
    out.append(line(140, 95, 140, 105, "F.SilkS"))
    out.append(line(150, 100, 160, 100, "F.SilkS"))
    out.append(line(155, 98, 155, 102, "F.SilkS"))
    # 6. A rule area on silk and mask over silk (never a collision, issue 24177).
    out.append(zone("", "F.SilkS", square(131, 104, 136, 109), [], rule_area=True))
    out.append(line(130, 106, 137, 106, "F.SilkS"))
    # 7. Silk arcs and a polygon.
    out.append(garc((100, 107), (102, 106), (104, 107), "F.SilkS"))
    out.append(poly([(103, 106.5), (106, 106.5), (106, 108), (103, 108)], "F.SilkS"))
    out.append(rect(95, 95, 145, 112, "Edge.Cuts", fill=False, width=0.1))
    return "".join(out)


def physical_items():
    out = []
    # 1. Tracks of different nets and the same net closer than the physical clearance.
    out.append(seg(100, 100, 108, 100, 0.25, "A"))
    out.append(seg(100, 100.8, 108, 100.8, 0.25, "B"))
    out.append(seg(108, 100, 110, 102, 0.25, "A"))
    out.append(seg(100, 103, 108, 103, 0.25, "C", "B.Cu"))
    # 2. Vias (holes) near tracks, graphics on tech layers (pierced by holes), each other.
    out.append(via(112, 100, 0.6, 0.3, "A"))
    out.append(via(113.2, 100, 0.6, 0.3, "B"))
    out.append(seg(111, 101.2, 115, 101.2, 0.2, "C"))
    out.append(line(111, 99.2, 115, 99.2, "F.SilkS", 0.12))
    out.append(line(111, 98.9, 115, 98.9, "B.Fab", 0.1))
    # 3. A track near the board edge (edge items pierce every physical layer).
    out.append(seg(100, 109.3, 108, 109.3, 0.25, "A"))
    # 4. A polygon on copper with a narrow notch, an arc, a hairpin bezier.
    out.append(poly([(116, 100), (122, 100), (122, 106), (119.4, 106), (119.4, 102),
                     (118.6, 102), (118.6, 106), (116, 106)], "F.Cu"))
    out.append(garc((124, 100), (126, 102), (124, 104), "F.Cu", 0.2))
    out.append(curve([(128, 100), (134, 100), (134, 101.2), (128, 101.2)], "F.Cu", 0.2))
    out.append(garc((128, 104), (129, 104.2), (130, 104), "B.Cu", 0.2))
    # 5. A zone with two fill islands close together and a fractured hole; a track and a via
    #    next to it.
    out.append(zone("D", "F.Cu", square(100, 112, 112, 120),
                    [[(100, 112), (105, 112), (105, 120), (100, 120), (100, 116), (102, 116),
                      (102, 118), (103, 118), (103, 114), (102, 114), (102, 116), (100, 116)],
                     [(105.5, 112), (112, 112), (112, 120), (105.5, 120)]]))
    out.append(seg(100, 121, 112, 121, 0.25, "A"))
    out.append(via(113, 116, 0.6, 0.3, "B"))
    out.append(text("T", 118, 110, "F.Cu", 1.0, 0.15))
    out.append(seg(116, 109.5, 120, 109.5, 0.2, "B"))
    out.append(rect(95, 95, 140, 125, "Edge.Cuts", fill=False, width=0.1))
    out.append(line(95, 110, 110, 110, "Edge.Cuts", 0.1))
    return "".join(out)


PHYS_DRU = ('(version 1)\n(rule phys\n\t(constraint physical_clearance (min {c}mm)))\n'
            '(rule physhole\n\t(constraint physical_hole_clearance (min {h}mm)))\n')

BOARDS = [
    # name, items, web width, silk clearance (board setup), rule file
    ("silk_default", silk_items, 0, 0, None),
    ("silk_setup", silk_items, 0, 0.2, None),
    ("silk_rule_web", silk_items, 0.25, 0,
     '(version 1)\n(rule silk\n\t(constraint silk_clearance (min 0.15mm)))\n'),
    ("physical", physical_items, 0, 0, PHYS_DRU.format(c=1, h=0.8)),
    ("physical_small", physical_items, 0, 0, PHYS_DRU.format(c=0.3, h=0.5)),
]


def write_boards():
    os.makedirs(DATA, exist_ok=True)

    for name, items, web, silk, dru in BOARDS:
        _uuid[0] = 0
        body = HEADER.format(web=web) + items() + ")\n"

        with open(os.path.join(DATA, name + ".kicad_pcb"), "w") as f:
            f.write(body)

        with open(os.path.join(DATA, name + ".kicad_pro"), "w") as f:
            f.write(PRO % (silk, name))

        if dru:
            with open(os.path.join(DATA, name + ".kicad_dru"), "w") as f:
                f.write(dru)


def run_reference(cli, name):
    with tempfile.TemporaryDirectory(prefix="drc26-") as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
            src = os.path.join(DATA, name + ext)

            if os.path.exists(src):
                shutil.copy(src, os.path.join(tmp, name + ext))

        out = os.path.join(tmp, "out.json")
        env = dict(os.environ)
        env.setdefault("KICAD_ORACLE_HOME", os.path.join(tmp, "home"))
        args = [cli, "pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]
        subprocess.run(args + ["-o", out, os.path.join(tmp, name + ".kicad_pcb")],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env, cwd=tmp,
                       timeout=600)

        with open(out) as f:
            return json.load(f)


def is_ours(v):
    t = v["type"]
    d = v["description"]

    if t == "silk_overlap":
        return True

    if t == "silk_over_copper":
        return len(v.get("items", [])) == 2

    if t in ("clearance", "hole_clearance"):
        return ("rule 'phys'" in d or "rule 'physhole'" in d
                or d.startswith("Internal clearance violation"))

    return False


def main():
    cli = "/home/user/kicad-oracle/bin/kicad-cli"

    if "--cli" in sys.argv:
        cli = sys.argv[sys.argv.index("--cli") + 1]

    write_boards()
    os.makedirs(GOLDEN, exist_ok=True)

    for name, *_ in BOARDS:
        report = run_reference(cli, name)
        lines = []

        for v in report.get("violations", []):
            if not is_ours(v):
                continue

            fields = [v["severity"], v["type"], v["description"]]

            for it in v.get("items", []):
                fields += [it["uuid"], it["description"], repr(it["pos"]["x"]),
                           repr(it["pos"]["y"])]

            lines.append("\t".join(f.replace("\t", " ").replace("\n", " ") for f in fields))

        with open(os.path.join(GOLDEN, name + ".txt"), "w") as f:
            f.write("".join(l + "\n" for l in lines))

        print(name, len(lines), "entries;", len(report.get("violations", [])),
              "violations in total")


if __name__ == "__main__":
    main()
