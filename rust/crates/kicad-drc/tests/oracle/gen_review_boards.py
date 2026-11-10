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

"""Generate the REV-DRC review boards of kicad-drc (tests/data/review).

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_review_boards.py OUT_DIR

Four copies of one small pad-less 4-layer board (tracks of nine nets on every copper layer,
through / blind / buried / micro vias, copper and silk graphics and texts, three copper zones
with different local clearances, two keepout rule areas), each with a project file (five
netclasses assigned by pattern, one net in two classes = a composite class) and:

    rev_implicit   no rule file; netclass DP has a diff-pair gap below its clearance, which
                   adds the implicit "diff pair" clearance override and disables the fast path
    rev_fastpath   no rule file, no override: the netclass clearance fast path
    rev_explicit   unconditional user rules of 20 constraint types (layer clauses, several
                   rules per type, severities, a value above MAXIMUM_CLEARANCE, disallow)
    rev_clearance  unconditional user clearance rules (per layer, clamped, opt only)

Their EvalRules results are compared with the C++ engine by
tests/evalrules_oracle.rs::evalrules_review_boards_match_cxx (golden:
gen_evalrules_goldens.py --out tests/golden/evalrules_review.txt).
"""
import json, os, sys

OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
uid = [0]

def u():
    uid[0] += 1
    return "00000000-0000-0000-0000-%012x" % uid[0]

def seg(x1, y1, x2, y2, w, layer, net):
    return f'''\t(segment
\t\t(start {x1} {y1})
\t\t(end {x2} {y2})
\t\t(width {w})
\t\t(layer "{layer}")
\t\t(net "{net}")
\t\t(uuid "{u()}")
\t)
'''

def via(kind, x, y, size, drill, l1, l2, net):
    k = f" {kind}" if kind else ""
    return f'''\t(via{k}
\t\t(at {x} {y})
\t\t(size {size})
\t\t(drill {drill})
\t\t(layers "{l1}" "{l2}")
\t\t(net "{net}")
\t\t(uuid "{u()}")
\t)
'''

def zone(net, layers, clearance, pts, keepout=None, name=None):
    ls = " ".join(f'"{l}"' for l in layers)
    ko = ""
    if keepout:
        ko = "\t\t(keepout\n" + "".join(
            f"\t\t\t({k} {'not_allowed' if v else 'allowed'})\n" for k, v in keepout) + "\t\t)\n"
    nm = f'\t\t(name "{name}")\n' if name else ""
    p = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f'''\t(zone
\t\t(net "{net}")
\t\t(layers {ls})
\t\t(uuid "{u()}")
{nm}\t\t(hatch edge 0.5)
\t\t(connect_pads
\t\t\t(clearance {clearance})
\t\t)
\t\t(min_thickness 0.25)
\t\t(filled_areas_thickness no)
{ko}\t\t(fill
\t\t\t(thermal_gap 0.5)
\t\t\t(thermal_bridge_width 0.5)
\t\t)
\t\t(polygon
\t\t\t(pts
\t\t\t\t{p}
\t\t\t)
\t\t)
\t)
'''

def gr_line(x1, y1, x2, y2, layer, w=0.15):
    return f'''\t(gr_line
\t\t(start {x1} {y1})
\t\t(end {x2} {y2})
\t\t(stroke
\t\t\t(width {w})
\t\t\t(type default)
\t\t)
\t\t(layer "{layer}")
\t\t(uuid "{u()}")
\t)
'''

def gr_rect(x1, y1, x2, y2, layer):
    return f'''\t(gr_rect
\t\t(start {x1} {y1})
\t\t(end {x2} {y2})
\t\t(stroke
\t\t\t(width 0.1)
\t\t\t(type default)
\t\t)
\t\t(fill no)
\t\t(layer "{layer}")
\t\t(uuid "{u()}")
\t)
'''

def gr_text(text, x, y, layer, h):
    return f'''\t(gr_text "{text}"
\t\t(at {x} {y} 0)
\t\t(layer "{layer}")
\t\t(uuid "{u()}")
\t\t(effects
\t\t\t(font
\t\t\t\t(size {h} {h})
\t\t\t\t(thickness 0.15)
\t\t\t)
\t\t)
\t)
'''

HEAD = '''(kicad_pcb
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
\t\t(5 "F.SilkS" user "F.Silkscreen")
\t\t(7 "B.SilkS" user "B.Silkscreen")
\t\t(25 "Edge.Cuts" user)
\t)
'''

def board():
    uid[0] = 0
    b = HEAD
    nets = ["/A", "/B", "/HV1", "/HV2", "/D_P", "/D_N", "/E3+", "/E3-", ""]
    y = 100
    for layer in ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]:
        for i, n in enumerate(nets):
            b += seg(100, y, 110, y, 0.2 + 0.05 * (i % 3), layer, n)
            y += 1
    b += via("", 120, 100, 0.6, 0.3, "F.Cu", "B.Cu", "/A")
    b += via("", 122, 100, 0.8, 0.4, "F.Cu", "B.Cu", "/HV1")
    b += via("blind", 124, 100, 0.5, 0.25, "F.Cu", "In2.Cu", "/B")
    b += via("micro", 126, 100, 0.3, 0.1, "F.Cu", "In1.Cu", "/D_P")
    b += via("blind", 128, 100, 0.5, 0.25, "In1.Cu", "In2.Cu", "/D_N")
    b += via("micro", 130, 100, 0.3, 0.1, "In2.Cu", "B.Cu", "/HV2")
    b += via("", 132, 100, 0.6, 0.3, "F.Cu", "B.Cu", "")
    b += gr_line(100, 150, 110, 150, "F.Cu")
    b += gr_line(100, 152, 110, 152, "In1.Cu")
    b += gr_rect(100, 154, 110, 158, "F.SilkS")
    b += gr_rect(90, 90, 200, 200, "Edge.Cuts")
    b += gr_text("COPPER", 105, 162, "F.Cu", 1.5)
    b += gr_text("SILK", 105, 166, "F.SilkS", 0.6)
    b += gr_text("BSILK", 105, 170, "B.SilkS", 2)
    b += zone("/A", ["F.Cu"], 0.6, [(140, 100), (150, 100), (150, 110), (140, 110)])
    b += zone("/HV1", ["F.Cu", "In1.Cu", "B.Cu"], 0.1, [(152, 100), (162, 100), (162, 110), (152, 110)])
    b += zone("", ["In2.Cu"], 0, [(164, 100), (174, 100), (174, 110), (164, 110)])
    # Keepouts far away from every other item, and none of them disallows zone fills: the
    # keepout predicate itself (`intersectsKeepout`) is package DRC-10, so no item may reach
    # the condition of a keepout rule (bounding boxes apart; for the zones, the keepouts
    # included, the disallow mask does not match).
    b += zone("", ["F.Cu", "B.Cu"], 0, [(180, 180), (190, 180), (190, 190), (180, 190)],
              keepout=[("tracks", 1), ("vias", 1), ("pads", 0), ("copperpour", 0), ("footprints", 0)],
              name="KO1")
    b += zone("", ["In1.Cu"], 0, [(192, 180), (198, 180), (198, 190), (192, 190)],
              keepout=[("tracks", 0), ("vias", 0), ("pads", 1), ("copperpour", 0), ("footprints", 1)])
    b += "\t(embedded_fonts no)\n)\n"
    return b

def project(name, classes, patterns, rules):
    return json.dumps({
        "board": {"design_settings": {"rules": rules}},
        "meta": {"filename": name + ".kicad_pro", "version": 3},
        "net_settings": {
            "classes": classes,
            "meta": {"version": 4},
            "net_colors": None,
            "netclass_assignments": None,
            "netclass_patterns": patterns,
        },
    }, indent=2) + "\n"

def nc(name, **kw):
    d = {"name": name, "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)",
         "priority": 0 if name != "Default" else 2147483647}
    if name == "Default":
        d.update({"bus_width": 12, "line_style": 0, "wire_width": 6, "clearance": 0.2, "track_width": 0.2,
                  "via_diameter": 0.6, "via_drill": 0.3, "microvia_diameter": 0.3, "microvia_drill": 0.1,
                  "diff_pair_width": 0.2, "diff_pair_gap": 0.25, "diff_pair_via_gap": 0.25})
    d.update(kw)
    return d

RULES = {"max_error": 0.005, "min_clearance": 0.13, "min_connection": 0.0, "min_copper_edge_clearance": 0.5,
         "min_hole_clearance": 0.25, "min_hole_to_hole": 0.25, "min_microvia_diameter": 0.2,
         "min_microvia_drill": 0.1, "min_resolved_spokes": 2, "min_silk_clearance": 0.0,
         "min_text_height": 0.8, "min_text_thickness": 0.08, "min_through_hole_diameter": 0.3,
         "min_track_width": 0.15, "min_via_annular_width": 0.1, "min_via_diameter": 0.5,
         "solder_mask_to_copper_clearance": 0.0, "use_height_for_length_calcs": True}

PATTERNS = [{"netclass": "HV", "pattern": "/HV*"}, {"netclass": "DP", "pattern": "/D_?"},
            {"netclass": "DP2", "pattern": "/E3*"}, {"netclass": "WIDE", "pattern": "/HV2"}]

def classes(dp_override):
    return [nc("Default"),
            nc("HV", clearance=0.5, track_width=0.4, via_diameter=0.9, via_drill=0.45),
            nc("DP", clearance=0.3, diff_pair_width=0.18, diff_pair_gap=0.12 if dp_override else 0.35,
               microvia_diameter=0.35, microvia_drill=0.15),
            nc("DP2", diff_pair_gap=0.22, track_width=0.3),
            nc("WIDE", clearance=0.5, track_width=0.6)]

DRU_EXPLICIT = '''(version 1)
(rule r_tw (constraint track_width (min 0.1mm) (max 2mm)))
(rule r_tw_opt (constraint track_width (opt 0.4mm)))
(rule r_inner (layer inner) (constraint track_width (min 0.3mm)))
(rule r_outer (layer outer) (constraint via_diameter (min 0.55mm)))
(rule r_bcu (layer "B.Cu") (constraint hole_size (min 0.35mm)))
(rule r_hs (severity ignore) (constraint hole_size (max 6mm)))
(rule r_h2h (constraint hole_to_hole (min 0.7mm)))
(rule r_hc (constraint hole_clearance (min 600mm)))
(rule r_edge (severity warning) (constraint edge_clearance (min 0.4mm)))
(rule r_disallow (constraint disallow blind_via micro_via graphic))
(rule r_dis_layer (layer "In1.Cu") (constraint disallow track zone text))
(rule r_zc (constraint zone_connection solid))
(rule r_trg (constraint thermal_relief_gap (min 0.33mm)))
(rule r_tsw (constraint thermal_spoke_width (opt 0.44mm)))
(rule r_dpgap (constraint diff_pair_gap (opt 0.2mm) (max 0.3mm)))
(rule r_phys (severity error) (constraint physical_clearance (min 0.25mm)))
(rule r_silk (layer "F.SilkS") (constraint silk_clearance (min 0.12mm)))
(rule r_th (constraint text_height (min 1mm) (max 3mm)))
(rule r_sme (constraint solder_mask_expansion (opt 0.07mm)))
(rule r_spa (constraint solder_paste_abs_margin (opt -0.02mm)))
(rule r_aw (constraint annular_width (min 0.2mm)))
(rule "dup name" (constraint via_diameter (max 0.7mm)))
(rule "dup name" (layer "F.Cu") (constraint via_diameter (min 0.45mm)))
'''

DRU_CLEARANCE = '''(version 1)
(rule r_clear_in1 (layer "In1.Cu") (constraint clearance (min 0.17mm)))
(rule r_clear_big (layer "B.Cu") (severity warning) (constraint clearance (min 700mm)))
(rule r_clear_opt (constraint clearance (opt 0.3mm)))
'''

for name, dp_override, dru in [("rev_implicit", True, None), ("rev_fastpath", False, None),
                               ("rev_explicit", True, DRU_EXPLICIT), ("rev_clearance", False, DRU_CLEARANCE)]:
    open(os.path.join(OUT, name + ".kicad_pcb"), "w").write(board())
    open(os.path.join(OUT, name + ".kicad_pro"), "w").write(project(name, classes(dp_override), PATTERNS, RULES))
    if dru:
        open(os.path.join(OUT, name + ".kicad_dru"), "w").write(dru)
print("ok")
