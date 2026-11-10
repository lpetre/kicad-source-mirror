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

"""DRC-25: synthetic thermal relief boards and their reference goldens.

Writes a board with GND zones on F.Cu and B.Cu and one single-pad footprint per test pad
(pads on and near the zone edges, spokes blocked by other-net tracks, a pocket that leaves a
single-connection island, a GND track and a GND copper line as manual spokes), lets the
reference fill the zones (`kicad-cli pcb drc --refill-zones --save-board` on a copy), stores
the filled board under `tests/data/zone_connections/`, and writes

  tests/data/zone_connections/<name>.pads   the pads of the board (the Rust parser does not
                                            read pads yet; the replay adds them through the
                                            model API): fp uuid, pad uuid, number, kind, x, y,
                                            w, h, net
  tests/golden/drc25/<name>.txt             the `starved_thermal` entries of
                                            `kicad-cli pcb drc --format json --severity-all
                                            --units mm` on the filled board, in report order

Run with `python3 -I`; the reference runs sequentially, on copies.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data", "zone_connections")
GOLDEN = os.path.join(HERE, "..", "golden", "drc25")

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
\t\t(13 "F.Paste" user)
\t\t(15 "B.Paste" user)
\t\t(25 "Edge.Cuts" user)
\t)
\t(setup
\t\t(pad_to_mask_clearance 0)
\t)
"""

PRO = """{
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
    return '\t(segment (start %s %s) (end %s %s) (width %s) (layer "%s") (net "%s") ' \
           '(uuid "%s"))\n' % (x1, y1, x2, y2, w, layer, net, uid())


def cu_line(x1, y1, x2, y2, w, net, layer="F.Cu"):
    return '\t(gr_line (start %s %s) (end %s %s) (stroke (width %s) (type solid)) ' \
           '(layer "%s") (net "%s") (uuid "%s"))\n' % (x1, y1, x2, y2, w, layer, net, uid())


def edge(x1, y1, x2, y2):
    return '\t(gr_rect (start %s %s) (end %s %s) (stroke (width 0.1) (type solid)) ' \
           '(fill no) (layer "Edge.Cuts") (uuid "%s"))\n' % (x1, y1, x2, y2, uid())


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
\t\t\t(island_removal_mode 1)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t%s
\t\t\t)
\t\t)
\t)
""" % (net, layer, uid(), pts)


# (kind, x, y, w, h, net): one footprint "TP<n>" at the origin per pad, so that the pad
# position is its board position. kind: smd (rect on F.Cu) or pth (circle, drill w / 2).
PADS = [
    ("smd", 20, 20, 1, 1, "GND"),         # inside the zone: 4 spokes
    ("smd", 10, 20, 1, 1, "GND"),         # on the left zone edge
    ("smd", 39.6, 29.6, 1, 1, "GND"),     # in the bottom right zone corner
    ("pth", 10.2, 25, 1.6, 1.6, "GND"),   # on the left edge, F.Cu and B.Cu
    ("smd", 30, 15, 1, 1, "GND"),         # three spokes blocked by SIG tracks
    ("smd", 18, 26, 1, 1, "GND"),         # inside a closed SIG ring: island only
    ("smd", 32, 25, 1, 1, "GND"),         # pocket above: one spoke to an island
    ("smd", 10, 12, 1, 1, "GND"),         # on the edge, GND track into the zone
    ("smd", 10, 16, 1, 1, "GND"),         # on the edge, GND copper line into the zone
    ("smd", 25, 20, 1, 1, "SIG"),         # other net: skipped
    ("pth", 39.8, 12, 2, 2, "GND"),       # PTH on the right edge
    ("smd", 9.6, 20, 1, 1, "GND"),        # just outside the zone: one spoke
    ("smd", 36, 25, 1, 1, "GND"),         # pocket above, left / right blocked
]


def footprint(n, kind, x, y, w, h, net):
    fp_uuid = uid()
    ref_uuid = uid()
    pad_uuid = uid()

    if kind == "smd":
        pad = '(pad "%d" smd rect (at %s %s) (size %s %s) (layers "F.Cu" "F.Mask" "F.Paste") ' \
              '(net "%s") (uuid "%s"))' % (n, x, y, w, h, net, pad_uuid)
    else:
        pad = '(pad "%d" thru_hole circle (at %s %s) (size %s %s) (drill %s) ' \
              '(layers "*.Cu" "*.Mask") (net "%s") (uuid "%s"))' \
              % (n, x, y, w, h, w / 2, net, pad_uuid)

    text = """\t(footprint "TP"
\t\t(layer "F.Cu")
\t\t(uuid "%s")
\t\t(at 0 0)
\t\t(property "Reference" "TP%d" (at %s %s 0) (layer "F.SilkS") (hide yes) (uuid "%s")
\t\t\t(effects (font (size 1 1) (thickness 0.15)))
\t\t)
\t\t%s
\t)
""" % (fp_uuid, n, x, y, ref_uuid, pad)

    return text, "\t".join(str(v) for v in (fp_uuid, pad_uuid, n, kind, x, y, w, h, net))


def board():
    _uuid[0] = 0
    body = HEADER
    pads = []

    for n, p in enumerate(PADS, start=1):
        text, line = footprint(n, *p)
        body += text
        pads.append(line)

    body += edge(0, 0, 50, 40)
    body += zone("GND", "F.Cu", 10, 10, 40, 30)
    body += zone("GND", "B.Cu", 10, 10, 40, 30)

    # Spokes of the pad at (30, 15) blocked on the left, right and top.
    body += seg(28.9, 13.5, 28.9, 16.2, 0.3, "SIG")
    body += seg(31.1, 13.5, 31.1, 16.2, 0.3, "SIG")
    body += seg(28.9, 13.5, 31.1, 13.5, 0.3, "SIG")

    # A closed ring around the pad at (18, 26).
    body += seg(16, 24, 20, 24, 0.3, "SIG")
    body += seg(20, 24, 20, 28, 0.3, "SIG")
    body += seg(20, 28, 16, 28, 0.3, "SIG")
    body += seg(16, 28, 16, 24, 0.3, "SIG")

    # A pocket above the pad at (32, 25), closed by the pad's thermal gap.
    body += seg(31.3, 25, 31.3, 22, 0.3, "SIG")
    body += seg(31.3, 22, 32.7, 22, 0.3, "SIG")
    body += seg(32.7, 22, 32.7, 25, 0.3, "SIG")

    # A pocket above the pad at (36, 25); its left and right spokes are blocked as well.
    body += seg(35.3, 26.5, 35.3, 22, 0.3, "SIG")
    body += seg(35.3, 22, 36.7, 22, 0.3, "SIG")
    body += seg(36.7, 22, 36.7, 26.5, 0.3, "SIG")

    # Manual spokes.
    body += seg(10, 12, 13, 12, 0.3, "GND")
    body += cu_line(10, 16, 13, 16, 0.3, "GND")

    return body + ")\n", pads


# (name, rule file): the second board requires 3 spokes through an unconditional rule (the
# message names the THERMAL_RELIEF_GAP constraint, a quirk of the C++).
BOARDS = [
    ("thermals", None),
    ("thermals_rule", """(version 1)
(rule "three_spokes"
\t(constraint min_resolved_spokes 3))
"""),
]


def run_reference(cli, src_dir, name, extra):
    """Runs the reference DRC on a copy of the board; returns (report, saved board text)."""
    with tempfile.TemporaryDirectory(prefix="drc25-") as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
            if os.path.exists(os.path.join(src_dir, name + ext)):
                shutil.copy(os.path.join(src_dir, name + ext), os.path.join(tmp, name + ext))

        out = os.path.join(tmp, "out.json")
        env = dict(os.environ)
        env.setdefault("KICAD_ORACLE_HOME", os.path.join(tmp, "home"))
        args = [cli, "pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]
        subprocess.run(args + extra + ["-o", out, os.path.join(tmp, name + ".kicad_pcb")],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env, cwd=tmp,
                       timeout=600)

        with open(out) as f:
            report = json.load(f)

        with open(os.path.join(tmp, name + ".kicad_pcb")) as f:
            return report, f.read()


def main():
    cli = "/home/user/kicad-oracle/bin/kicad-cli"

    if "--cli" in sys.argv:
        cli = sys.argv[sys.argv.index("--cli") + 1]

    os.makedirs(DATA, exist_ok=True)
    os.makedirs(GOLDEN, exist_ok=True)

    for name, dru in BOARDS:
        generate(cli, name, dru)


def generate(cli, name, dru):
    body, pads = board()

    with tempfile.TemporaryDirectory(prefix="drc25-src-") as src:
        with open(os.path.join(src, name + ".kicad_pcb"), "w") as f:
            f.write(body)

        with open(os.path.join(src, name + ".kicad_pro"), "w") as f:
            f.write(PRO % name)

        # Fill the zones with the reference and keep the saved board.
        _, filled = run_reference(cli, src, name, ["--refill-zones", "--save-board"])

    with open(os.path.join(DATA, name + ".kicad_pcb"), "w") as f:
        f.write(filled)

    with open(os.path.join(DATA, name + ".kicad_pro"), "w") as f:
        f.write(PRO % name)

    if dru:
        with open(os.path.join(DATA, name + ".kicad_dru"), "w") as f:
            f.write(dru)

    with open(os.path.join(DATA, name + ".pads"), "w") as f:
        f.write("\n".join(pads) + "\n")

    report, _ = run_reference(cli, DATA, name, [])
    lines = []

    for v in report.get("violations", []):
        if v["type"] != "starved_thermal":
            continue

        fields = [v["severity"], v["type"], v["description"]]

        for it in v.get("items", []):
            fields += [it["uuid"], it["description"], repr(it["pos"]["x"]),
                       repr(it["pos"]["y"])]

        lines.append("\t".join(f.replace("\t", " ").replace("\n", " ") for f in fields))

    with open(os.path.join(GOLDEN, name + ".txt"), "w") as f:
        f.write("".join(line + "\n" for line in lines))

    print("%s: %d starved_thermal entries" % (name, len(lines)))


if __name__ == "__main__":
    main()
