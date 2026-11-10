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

"""DRC-27 (solder mask provider): synthetic boards without footprints and their reference
violations.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc27_boards.py [--cli KICAD_CLI]

Writes tests/data/solder_mask/*.kicad_pcb (+ .kicad_pro / .kicad_dru) and, for each board, runs
the reference on a copy:

    kicad-cli pcb drc --format json --severity-all --units mm [--all-track-errors]

and stores the `solder_mask_bridge` entries and the one-item `silk_over_copper` entries (the
two-item ones come from the silk clearance provider) in report order as
tests/golden/drc27/<board>[_all].txt, one entry per line:

    <severity> TAB <type> TAB <description> { TAB <uuid> TAB <item description> TAB <x> TAB <y> }
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data", "solder_mask")
GOLDEN = os.path.join(HERE, "..", "golden", "drc27")

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
\t\t(5 "F.SilkS" user "F.Silkscreen")
\t\t(7 "B.SilkS" user "B.Silkscreen")
\t\t(1 "F.Mask" user)
\t\t(3 "B.Mask" user)
\t\t(25 "Edge.Cuts" user)
\t)
\t(setup
\t\t(pad_to_mask_clearance {expansion})
\t\t(solder_mask_min_width {web})
\t\t(allow_soldermask_bridges_in_footprints no)
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
        "solder_mask_to_copper_clearance": %s
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


def seg(x1, y1, x2, y2, w, net, layers=("F.Cu",), margin=None):
    lay = '(layer "%s")' % layers[0] if len(layers) == 1 else \
        "(layers %s)" % " ".join('"%s"' % l for l in layers)
    m = "" if margin is None else " (solder_mask_margin %s)" % margin
    return '\t(segment (start %s %s) (end %s %s) (width %s) %s%s (net "%s") (uuid "%s"))\n' % (
        x1, y1, x2, y2, w, lay, m, net, uid())


def arc(s, m, e, w, net, layers):
    return '\t(arc (start %s %s) (mid %s %s) (end %s %s) (width %s) (layers %s) (net "%s") (uuid "%s"))\n' % (
        s[0], s[1], m[0], m[1], e[0], e[1], w, " ".join('"%s"' % l for l in layers), net, uid())


def via(x, y, size, drill, net, tented):
    t = "" if tented else " (tenting (front no) (back no))"
    return '\t(via (at %s %s) (size %s) (drill %s) (layers "F.Cu" "B.Cu")%s (net "%s") (uuid "%s"))\n' % (
        x, y, size, drill, t, net, uid())


def rect(x1, y1, x2, y2, layer, fill=True, width=0.05):
    return ('\t(gr_rect (start %s %s) (end %s %s) (stroke (width %s) (type solid)) (fill %s) '
            '(layer "%s") (uuid "%s"))\n') % (x1, y1, x2, y2, width, "yes" if fill else "no", layer, uid())


def circle(cx, cy, r, layer, fill=True, width=0.05):
    return ('\t(gr_circle (center %s %s) (end %s %s) (stroke (width %s) (type solid)) (fill %s) '
            '(layer "%s") (uuid "%s"))\n') % (cx, cy, cx + r, cy, width, "yes" if fill else "no", layer, uid())


def line(x1, y1, x2, y2, layer, width=0.15):
    return ('\t(gr_line (start %s %s) (end %s %s) (stroke (width %s) (type solid)) (layer "%s") '
            '(uuid "%s"))\n') % (x1, y1, x2, y2, width, layer, uid())


def text(s, x, y, layer, size=1.0, thickness=0.15, mirror=False):
    j = " (justify mirror)" if mirror else ""
    return ('\t(gr_text "%s" (at %s %s 0) (layer "%s") (uuid "%s") (effects (font (size %s %s) '
            '(thickness %s))%s))\n') % (s, x, y, layer, uid(), size, size, thickness, j)


def zone(net, layer, x1, y1, x2, y2):
    pts = "(xy %s %s) (xy %s %s) (xy %s %s) (xy %s %s)" % (x1, y1, x2, y1, x2, y2, x1, y2)
    return """\t(zone
\t\t(net "%s")
\t\t(layer "%s")
\t\t(uuid "%s")
\t\t(hatch edge 0.5)
\t\t(connect_pads
\t\t\t(clearance 0.25)
\t\t)
\t\t(min_thickness 0.25)
\t\t(fill yes
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t\t(island_removal_mode 0)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t%s
\t\t\t)
\t\t)
\t\t(filled_polygon
\t\t\t(layer "%s")
\t\t\t(pts
\t\t\t\t%s
\t\t\t)
\t\t)
\t)
""" % (net, layer, uid(), pts, layer, pts)


def board_items():
    out = []
    # 1. A mask-only aperture exposing plain copper of three nets (aperture bridges, track
    #    throttling without --all-track-errors).
    out.append(rect(100, 100, 106, 104, "F.Mask"))
    out.append(seg(99, 101, 107, 101, 0.25, "A"))
    out.append(seg(99, 101.6, 107, 101.6, 0.25, "A"))
    out.append(seg(99, 102.4, 107, 102.4, 0.25, "B"))
    out.append(seg(99, 103.2, 103, 103.2, 0.25, "C"))
    # 2. Exposed tracks (F.Cu + F.Mask) of two nets closer than the web width.
    out.append(seg(110, 100, 118, 100, 0.25, "A", ("F.Cu", "F.Mask")))
    out.append(seg(110, 100.45, 118, 100.45, 0.25, "B", ("F.Cu", "F.Mask"), margin=0.02))
    out.append(arc((110, 102), (114, 101.0), (118, 102), 0.25, "B", ("F.Cu", "F.Mask")))
    # 3. Untented vias of two nets (both sides).
    out.append(via(120, 110, 0.8, 0.4, "A", False))
    out.append(via(121, 110, 0.8, 0.4, "C", False))
    out.append(via(123, 110, 0.8, 0.4, "C", True))
    # 4. A copper zone and an exposed track / untented via of other nets next to it.
    out.append(zone("C", "F.Cu", 130, 100, 136, 106))
    out.append(seg(129, 106.3, 137, 106.3, 0.25, "A", ("F.Cu", "F.Mask")))
    out.append(via(133, 107.2, 0.8, 0.4, "B", False))
    out.append(rect(131, 98, 135, 101, "F.Mask"))
    out.append(seg(130, 99, 136, 99, 0.25, "B"))
    out.append(seg(129, 105.9, 137, 105.9, 0.25, "B", ("F.Cu", "F.Mask")))
    # 5. Back side: a filled circle on B.Mask over two nets, a mask zone over one.
    out.append(circle(140, 110, 2, "B.Mask"))
    out.append(seg(137, 109.5, 143, 109.5, 0.3, "A", ("B.Cu",)))
    out.append(seg(137, 110.5, 143, 110.5, 0.3, "B", ("B.Cu",)))
    out.append(zone("", "B.Mask", 145, 108, 149, 112))
    out.append(seg(144, 109, 150, 109, 0.3, "A", ("B.Cu",)))
    out.append(seg(144, 111, 150, 111, 0.3, "C", ("B.Cu",)))
    # 6. Silkscreen over mask openings.
    out.append(line(100, 100.5, 106, 100.5, "F.SilkS"))
    out.append(line(108, 99, 108, 105, "F.SilkS"))
    out.append(text("SILK", 114, 100.2, "F.SilkS", 0.8, 0.12))
    out.append(line(138, 108, 142, 112, "B.SilkS"))
    out.append(text("BK", 147, 110, "B.SilkS", 1.0, 0.15, mirror=True))
    out.append(circle(121, 110, 0.3, "F.SilkS", fill=False, width=0.1))
    # 7. Text as a mask aperture next to copper of two nets.
    out.append(text("M", 125, 103, "F.Mask", 1.5, 0.3))
    out.append(seg(124, 102.6, 126, 102.6, 0.2, "A"))
    out.append(seg(124, 103.4, 126, 103.4, 0.2, "B"))
    # Board outline.
    out.append(rect(95, 95, 155, 115, "Edge.Cuts", fill=False, width=0.1))
    return "".join(out)


BOARDS = [
    # name, expansion, web width, mask-to-copper clearance, rule file
    ("mask_web", 0.05, 0.25, 0.2, None),
    ("mask_noweb", 0, 0, 0, None),
    ("mask_ignore_front", 0.05, 0.25, 0.1,
     '(version 1)\n(rule ignore_front_mask\n\t(layer "F.Mask")\n\t(constraint bridged_mask)\n'
     '\t(severity ignore))\n(rule silk\n\t(constraint silk_clearance (min 0.1mm)))\n'),
]


def write_boards():
    os.makedirs(DATA, exist_ok=True)

    for name, expansion, web, m2c, dru in BOARDS:
        _uuid[0] = 0
        body = HEADER.format(expansion=expansion, web=web) + board_items() + ")\n"

        with open(os.path.join(DATA, name + ".kicad_pcb"), "w") as f:
            f.write(body)

        with open(os.path.join(DATA, name + ".kicad_pro"), "w") as f:
            f.write(PRO % (m2c, name))

        if dru:
            with open(os.path.join(DATA, name + ".kicad_dru"), "w") as f:
                f.write(dru)


def run_reference(cli, name, all_tracks):
    with tempfile.TemporaryDirectory(prefix="drc27-") as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
            src = os.path.join(DATA, name + ext)

            if os.path.exists(src):
                shutil.copy(src, os.path.join(tmp, name + ext))

        out = os.path.join(tmp, "out.json")
        env = dict(os.environ)
        env.setdefault("KICAD_ORACLE_HOME", os.path.join(tmp, "home"))
        args = [cli, "pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]

        if all_tracks:
            args.append("--all-track-errors")

        subprocess.run(args + ["-o", out, os.path.join(tmp, name + ".kicad_pcb")],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env, cwd=tmp,
                       timeout=600)

        with open(out) as f:
            return json.load(f)


def main():
    cli = "/home/user/kicad-oracle/bin/kicad-cli"

    if "--cli" in sys.argv:
        cli = sys.argv[sys.argv.index("--cli") + 1]

    write_boards()
    os.makedirs(GOLDEN, exist_ok=True)

    for name, *_ in BOARDS:
        for all_tracks in (False, True):
            report = run_reference(cli, name, all_tracks)
            lines = []

            for v in report.get("violations", []):
                items = v.get("items", [])

                if v["type"] != "solder_mask_bridge" and not (
                        v["type"] == "silk_over_copper" and len(items) == 1):
                    continue

                fields = [v["severity"], v["type"], v["description"]]

                for it in items:
                    fields += [it["uuid"], it["description"], repr(it["pos"]["x"]),
                               repr(it["pos"]["y"])]

                lines.append("\t".join(f.replace("\t", " ").replace("\n", " ") for f in fields))

            suffix = "_all" if all_tracks else ""

            with open(os.path.join(GOLDEN, name + suffix + ".txt"), "w") as f:
                f.write("".join(l + "\n" for l in lines))

            print(name + suffix, len(lines), "entries;",
                  len(report.get("violations", [])), "violations in total")


if __name__ == "__main__":
    main()
