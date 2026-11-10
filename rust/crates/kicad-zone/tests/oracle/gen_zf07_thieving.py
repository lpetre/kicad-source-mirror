# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Package ZF-07: writes the crafted copper-thieving boards (every pattern kind and parameter
# combination of ZONE_FILLER::addCopperThievingPattern) into the directory given as argument.
# They are refilled by the reference through tools/zone-corpus/run.sh:
#
#   python3 -I crates/kicad-zone/tests/oracle/gen_zf07_thieving.py target/zf07-boards
#   tools/zone-corpus/run.sh target/zf07-boards/*.kicad_pcb
#
# which caches the reference refill under target/zone-corpus/ where tests/zone_fill_corpus.rs
# (fills) and kicad-session tests/zf06_refill_save.rs (saved boards) compare against it.
import os
import sys

HEADER = """(kicad_pcb (version 20260513) (generator "pcbnew") (generator_version "10.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user "F.Silkscreen")
    (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0){defaults})
  (net 0 "") (net 1 "GND")
  (gr_rect (start -60 -60) (end 200 200) (layer "Edge.Cuts")
    (uuid "aaaaaaaa-0000-0000-0000-000000000001") (stroke (width 0.05) (type default)))
{items}
)
"""


def zone(n, kind, size, gap, width, stagger, orient, pts, layers='"F.Cu"', min_thickness=0.25,
         props="", clearance=0.5):
    pts_text = " ".join(f"(xy {x} {y})" for x, y in pts)
    layer_kw = "layers" if " " in layers else "layer"
    return f"""  (zone ({layer_kw} {layers}) (uuid "bbbbbbbb-0000-0000-0000-{n:012}")
    (hatch edge 0.5) (connect_pads (clearance {clearance})) (min_thickness {min_thickness})
    (fill yes (mode thieving) (thermal_gap 0.5) (thermal_bridge_width 0.5)
      (island_removal_mode 0)
      (thieving (type {kind}) (size {size}) (gap {gap}) (width {width})
        (stagger {"yes" if stagger else "no"}) (orientation {orient})))
    {props}
    (polygon (pts {pts_text})))"""


def rect(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def track(n, x0, y0, x1, y1, width=0.4, layer="F.Cu"):
    return f"""  (segment (start {x0} {y0}) (end {x1} {y1}) (width {width}) (layer "{layer}")
    (net "GND") (uuid "cccccccc-0000-0000-0000-{n:012}"))"""


def via(n, x, y):
    return f"""  (via (at {x} {y}) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net "GND")
    (uuid "dddddddd-0000-0000-0000-{n:012}"))"""


R = rect(100, 100, 120, 120)
POLY = [(100, 100), (125, 103), (131.7, 118.3), (112, 126.5), (98.2, 115)]
OFFSET = '(property (layer "F.Cu") (hatch_position (xy 0.3 0.7)))'
DEFAULTS = (' (zone_defaults (property (layer "F.Cu") (hatch_position (xy -0.45 1.1)))'
            ' (property (layer "B.Cu") (hatch_position (xy 0.2 0.25))))')

CASES = {
    "dots_plain": [zone(1, "dots", 0.5, 2, 0.3, False, 0, R)],
    "dots_stagger": [zone(1, "dots", 0.5, 2, 0.3, True, 0, R)],
    "dots_rot30_stagger": [zone(1, "dots", 0.5, 2, 0.3, True, 30, R)],
    "dots_small": [zone(1, "dots", 0.2, 0.9, 0.3, False, 0, R)],
    "dots_odd_stride": [zone(1, "dots", 0.55, 1.333, 0.3, True, 17.5, POLY, min_thickness=0.2)],
    "dots_negative": [zone(1, "dots", 0.6, 1.7, 0.3, True, 0, rect(-37.3, -41.1, -12.2, -15.9))],
    "dots_negative_rot": [zone(1, "dots", 0.6, 1.7, 0.3, False, 135,
                               rect(-37.3, -41.1, -12.2, -15.9))],
    "dots_local_offset": [zone(1, "dots", 0.5, 2, 0.3, False, 0, R, props=OFFSET)],
    "dots_obstacles": [zone(1, "dots", 0.5, 1.5, 0.3, False, 0, R),
                       track(1, 95, 107.3, 125, 113.1), via(1, 104.2, 117.7)],
    "dots_two_layers": [zone(1, "dots", 0.5, 1.2, 0.3, True, 0, R, layers='"F.Cu" "B.Cu"')],
    "dots_silk": [zone(1, "dots", 0.5, 1.5, 0.3, False, 0, R, layers='"F.SilkS"')],
    "dots_dense": [zone(1, "dots", 0.3, 1.0, 0.3, False, 0, rect(100, 100, 150, 150))],
    "squares_plain": [zone(1, "squares", 0.6, 2, 0.3, False, 0, R)],
    "squares_rot45_stagger": [zone(1, "squares", 0.6, 1.4, 0.3, True, 45, R)],
    "squares_small": [zone(1, "squares", 0.2, 1.0, 0.3, False, 0, POLY)],
    "squares_obstacles_rot90": [zone(1, "squares", 0.8, 1.1, 0.3, True, 90, R),
                                track(1, 99, 99, 121, 121, 0.3), via(1, 113, 104)],
    "squares_board_offset": [zone(1, "squares", 0.7, 1.3, 0.3, False, 0, R,
                                  layers='"F.Cu" "B.Cu"')],
    "hatch_plain": [zone(1, "hatch", 5, 2, 0.3, False, 0, R)],
    "hatch_rot30": [zone(1, "hatch", 5, 1.2, 0.5, False, 30, R)],
    "hatch_poly_offset": [zone(1, "hatch", 5, 1.0, 0.4, True, 0, POLY, props=OFFSET)],
    "hatch_track_rot60": [zone(1, "hatch", 5, 1.5, 0.35, False, 60, POLY),
                          track(1, 97, 112, 133, 114), via(1, 120, 108)],
    "hatch_negative": [zone(1, "hatch", 5, 1.7, 0.6, False, 0, rect(-37.3, -41.1, -12.2, -15.9))],
    "hatch_narrow": [zone(1, "hatch", 5, 2, 3, False, 0, rect(100, 100, 105, 103))],
    "gap_zero": [zone(1, "dots", 0.5, 0, 0.3, False, 0, R)],
    "size_zero_squares": [zone(1, "squares", 0, 1, 0.3, False, 0, R)],
}

# Boards that use the board-default (zone_defaults) layer offsets.
WITH_DEFAULTS = {"squares_board_offset", "hatch_rot30", "dots_two_layers"}


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)

    for name, items in CASES.items():
        defaults = DEFAULTS if name in WITH_DEFAULTS else ""
        with open(os.path.join(out, f"zf07_{name}.kicad_pcb"), "w") as f:
            f.write(HEADER.format(defaults=defaults, items="\n".join(items)))


if __name__ == "__main__":
    main()
