#!/usr/bin/env python3
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
"""Generator of the crafted zone-fill boards of package REV-ZF.

Every board exercises branches of pcbnew/zone_filler.cpp that no qa/data board of the zone
corpus reaches (see rust/docs/design/review-zone-fill.md, coverage map).  The boards are
written to tests/data/rev_zf/ (inputs, committed); make_rev_zf_goldens.sh refills copies with
the reference kicad-cli and tests/rev_zf_crafted.rs compares the port with the goldens.

Usage: gen_rev_zf.py OUT_DIR
"""
import os
import sys

X0, Y0, W, H = 100.0, 100.0, 40.0, 30.0


class Board:
    def __init__(self, name, layers=2, edge=True, zone_defaults=""):
        self.name = name
        self.n = 0
        self.items = []
        self.layers = layers
        self.zone_defaults = zone_defaults
        self.side = {}
        if edge:
            self.rect(X0, Y0, X0 + W, Y0 + H, "Edge.Cuts", 0.05)

    def uuid(self):
        self.n += 1
        return f"00000000-0000-4000-8000-{self.n:012x}"

    def add(self, s):
        self.items.append(s)

    # ---- graphics ---------------------------------------------------------------------
    def rect(self, x1, y1, x2, y2, layer, width=0.1, fill="no", net=None):
        net_s = f' (net "{net}")' if net else ""
        self.add(f'(gr_rect (start {x1} {y1}) (end {x2} {y2}) (stroke (width {width}) (type default)) '
                 f'(fill {fill}) (layer "{layer}"){net_s} (uuid "{self.uuid()}"))')

    def line(self, x1, y1, x2, y2, layer, width=0.1, net=None):
        net_s = f' (net "{net}")' if net else ""
        self.add(f'(gr_line (start {x1} {y1}) (end {x2} {y2}) (stroke (width {width}) (type default)) '
                 f'(layer "{layer}"){net_s} (uuid "{self.uuid()}"))')

    def circle(self, cx, cy, r, layer, width=0.1, fill="no"):
        self.add(f'(gr_circle (center {cx} {cy}) (end {cx + r} {cy}) (stroke (width {width}) (type default)) '
                 f'(fill {fill}) (layer "{layer}") (uuid "{self.uuid()}"))')

    def arc(self, sx, sy, mx, my, ex, ey, layer, width=0.1):
        self.add(f'(gr_arc (start {sx} {sy}) (mid {mx} {my}) (end {ex} {ey}) (stroke (width {width}) (type default)) '
                 f'(layer "{layer}") (uuid "{self.uuid()}"))')

    def text(self, s, x, y, layer, knockout=False, size=1.5, rot=0):
        ko = " knockout" if knockout else ""
        self.add(f'(gr_text "{s}" (at {x} {y} {rot}) (layer "{layer}"{ko}) (uuid "{self.uuid()}") '
                 f'(effects (font (size {size} {size}) (thickness {size * 0.15:.4g}))))')

    # ---- tracks -----------------------------------------------------------------------
    def seg(self, x1, y1, x2, y2, layer, net, width=0.25):
        self.add(f'(segment (start {x1} {y1}) (end {x2} {y2}) (width {width}) (layer "{layer}") '
                 f'(net "{net}") (uuid "{self.uuid()}"))')

    def via(self, x, y, net, size=0.6, drill=0.3, extra="", layers=("F.Cu", "B.Cu"), kind=""):
        k = f" {kind}" if kind else ""
        self.add(f'(via{k} (at {x} {y}) (size {size}) (drill {drill}) {extra} (layers "{layers[0]}" "{layers[1]}") '
                 f'(net "{net}") (uuid "{self.uuid()}"))')

    # ---- footprints -------------------------------------------------------------------
    def pad(self, num, kind, shape, at, size, net, layers, rot=0, drill=None, extra=""):
        d = f" (drill {drill})" if drill is not None else ""
        net_s = f' (net "{net}")' if net else ""
        ls = " ".join(f'"{l}"' for l in layers)
        r = f" {rot}" if rot else ""
        return (f'(pad "{num}" {kind} {shape} (at {at[0]} {at[1]}{r}) (size {size[0]} {size[1]}){d} '
                f'(layers {ls}){net_s} {extra} (uuid "{self.uuid()}"))')

    def footprint(self, ref, x, y, body, rot=0, layer="F.Cu", attr="smd", zone_connect=None, extra=""):
        r = f" {rot}" if rot else ""
        zc = f"(zone_connect {zone_connect})" if zone_connect is not None else ""
        silk = "F.SilkS" if layer == "F.Cu" else "B.SilkS"
        fab = "F.Fab" if layer == "F.Cu" else "B.Fab"
        self.add(
            f'(footprint "rev:{ref}" (layer "{layer}") (uuid "{self.uuid()}") (at {x} {y}{r})\n'
            f'  (property "Reference" "{ref}" (at 0 -4{r}) (layer "{silk}") (hide yes) (uuid "{self.uuid()}") '
            f'(effects (font (size 1 1) (thickness 0.15))))\n'
            f'  (property "Value" "V" (at 0 4{r}) (layer "{fab}") (hide yes) (uuid "{self.uuid()}") '
            f'(effects (font (size 1 1) (thickness 0.15))))\n'
            f'  {zc} (attr {attr}) {extra}\n  ' + "\n  ".join(body) + "\n)")

    # ---- zones ------------------------------------------------------------------------
    def zone_str(self, pts, layers, net=None, name="", priority=None, connect="", clearance=0.3,
                 min_thickness=0.25, fill="", keepout=None, extra="", uuid=None, tgap=0.5, tw=0.5,
                 with_uuid=True, raw_pts=None):
        if isinstance(layers, str):
            ls = f'(layer "{layers}")'
        else:
            ls = "(layers " + " ".join(f'"{l}"' for l in layers) + ")"
        net_s = f'(net "{net}") ' if net is not None else ""
        pr = f"(priority {priority}) " if priority is not None else ""
        ko = ""
        if keepout:
            ko = ("(keepout (tracks allowed) (vias allowed) (pads allowed) "
                  f"(copperpour {keepout}) (footprints allowed)) ")
        p = raw_pts if raw_pts else " ".join(f"(xy {x} {y})" for x, y in pts)
        u = f'(uuid "{uuid or self.uuid()}") ' if with_uuid else ""
        return (f'(zone {net_s}{ls} {u}(name "{name}") (hatch edge 0.5) {pr}'
                f'(connect_pads {connect} (clearance {clearance})) (min_thickness {min_thickness}) {ko}'
                f'(fill yes (thermal_gap {tgap}) (thermal_bridge_width {tw}) {fill}) {extra}'
                f'(polygon (pts {p})))')

    def zone(self, *a, **k):
        self.add(self.zone_str(*a, **k))

    # ---- output -----------------------------------------------------------------------
    def write(self, out_dir):
        cu = [(0, "F.Cu")]
        if self.layers == 4:
            cu += [(4, "In1.Cu"), (6, "In2.Cu")]
        cu += [(2, "B.Cu")]
        lay = "\n".join(f'    ({i} "{n}" signal)' for i, n in cu)
        zd = f"(zone_defaults {self.zone_defaults})" if self.zone_defaults else ""
        text = f'''(kicad_pcb (version 20260101) (generator "rev_zf") (generator_version "9.99")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers
{lay}
    (5 "F.SilkS" user "F.Silkscreen")
    (7 "B.SilkS" user "B.Silkscreen")
    (1 "F.Mask" user)
    (3 "B.Mask" user)
    (17 "Dwgs.User" user "User.Drawings")
    (25 "Edge.Cuts" user)
    (27 "Margin" user)
    (31 "F.CrtYd" user "F.Courtyard")
    (29 "B.CrtYd" user "B.Courtyard")
    (35 "F.Fab" user)
    (33 "B.Fab" user)
  )
  (setup (pad_to_mask_clearance 0) {zd})
''' + "\n".join("  " + i for i in self.items) + "\n)\n"
        with open(os.path.join(out_dir, self.name + ".kicad_pcb"), "w") as f:
            f.write(text)
        for ext, content in self.side.items():
            with open(os.path.join(out_dir, self.name + "." + ext), "w") as f:
                f.write(content)


def box(x1, y1, x2, y2):
    return [(x1, y1), (x2, y1), (x2, y2), (x1, y2)]


FULL = box(X0 + 1, Y0 + 1, X0 + W - 1, Y0 + H - 1)
SMD = ("F.Cu", "F.Mask")
THT = ("*.Cu", "*.Mask")


def custom_spokes():
    """buildThermalSpokes customSpokes branch, addKnockout convex hull."""
    b = Board("custom_spokes")
    b.zone(FULL, "F.Cu", net="A", name="ZA")
    prim = ("(primitives (gr_poly (pts (xy -2 -1) (xy 2 -1) (xy 2 1) (xy -2 1)) (width 0) (fill yes)) "
            "(gr_vector (start 0 0) (end 3.5 0)) (gr_vector (start 0 0) (end -3.5 0)) "
            "(gr_vector (start 0 0) (end 0 2.6)) (gr_vector (start 0 -2.6) (end 0 0)) "
            "(gr_vector (start 0.5 0.3) (end 3 2.5)) (gr_vector (start 5 5) (end 6 6)))")
    for ref, x, y, rot in (("U1", 108, 108, 0), ("U2", 122, 108, 30), ("U3", 133, 108, 270)):
        b.footprint(ref, x, y, [b.pad("1", "smd", "custom", (0, 0), (1, 1), "A", SMD, rot=rot,
                                      extra="(options (clearance outline) (anchor rect)) " + prim)], rot=rot)
    # concave custom pads, other net: outline and convex hull clearance
    lshape = "(primitives (gr_poly (pts (xy -2 -2) (xy 2 -2) (xy 2 -1) (xy -1 -1) (xy -1 2) (xy -2 2)) (width 0.2) (fill yes)))"
    lvalid = "(primitives (gr_poly (pts (xy -2 -2) (xy 2 -2) (xy 2 0.3) (xy 0.3 0.3) (xy 0.3 2) (xy -2 2)) (width 0.2) (fill yes)))"
    b.footprint("U4", 108, 121, [b.pad("1", "smd", "custom", (0, 0), (1, 1), "B", SMD,
                                       extra="(options (clearance convexhull) (anchor circle)) " + lvalid)])
    b.footprint("U5", 118, 121, [b.pad("1", "smd", "custom", (0, 0), (1, 1), "B", SMD, rot=45,
                                       extra="(options (clearance outline) (anchor circle)) " + lshape)], rot=45)
    # same net, convex hull thermal knockout, no spoke templates (bounding box spokes)
    b.footprint("U6", 128, 121, [b.pad("1", "smd", "custom", (0, 0), (1, 1), "A", SMD, rot=20,
                                       extra="(thermal_bridge_angle 45) (options (clearance convexhull) (anchor rect)) " + lvalid)],
                rot=20)
    # custom pad with a hole and a shape offset
    b.footprint("U7", 135, 121, [b.pad("1", "thru_hole", "custom", (0, 0), (1.6, 1.6), "A", THT, drill="0.8 (offset 0.4 0)",
                                       extra="(options (clearance outline) (anchor circle)) "
                                             "(primitives (gr_circle (center 0 1) (end 0.8 1) (width 0.1) (fill yes)) "
                                             "(gr_vector (start 0 0) (end 2.5 0)) (gr_vector (start 0 1) (end 0 3)))")],
                attr="through_hole")
    return b


def hatch_vias():
    """Hatch fills: via thermals / full connections, rings of every pad shape, smoothing 2 / 3."""
    b = Board("hatch_vias", zone_defaults='(property (layer "B.Cu") (hatch_position (xy 0.35 0.2)))')
    hf = ("(mode hatch) (island_removal_mode 1) (hatch_thickness 0.4) (hatch_gap 1.6) (hatch_orientation 30) "
          "(hatch_smoothing_level 2) (hatch_smoothing_value 0.6) (hatch_border_algorithm hatch_thickness) "
          "(hatch_min_hole_area 0.3)")
    hb = ("(mode hatch) (island_removal_mode 1) (hatch_thickness 0.5) (hatch_gap 2.2) (hatch_orientation 0) "
          "(hatch_smoothing_level 3) (hatch_smoothing_value 1) (hatch_border_algorithm hatch_thickness) "
          "(hatch_min_hole_area 0.15)")
    b.zone(FULL, "F.Cu", net="A", name="HF", fill=hf,
           extra='(property (layer "F.Cu") (hatch_position (xy 0.3 -0.45))) ')
    b.zone(FULL, "B.Cu", net="A", name="HB", connect="yes", fill=hb)
    for i, (x, y) in enumerate(((104.3, 104.1), (106.9, 104.6), (109.2, 105.5), (111.8, 104.2), (114.4, 105.9),
                                (117.1, 104.4), (119.6, 105.2), (104.9, 108.3), (107.7, 109.1), (110.3, 108.2))):
        b.via(x, y, "A")
    b.via(124, 105, "A", size=2.4, drill=1.2)
    b.via(128, 105, "B")
    b.via(131, 105.5, "", size=0.8, drill=0.4)
    b.via(134, 105, "A", size=0.24, drill=0.1)  # narrower than the zone min thickness
    b.seg(128, 105, 137, 105, "F.Cu", "B")
    pads = [
        ("P1", 105, 115, 0, [("1", "thru_hole", "circle", (1.8, 1.8), "A", THT, "0.9", "")]),
        ("P2", 110, 115, 0, [("1", "smd", "rect", (2.2, 1.2), "A", SMD, None, "")]),
        ("P3", 115, 115, 25, [("1", "smd", "roundrect", (2.2, 1.4), "A", SMD, None, "(roundrect_rratio 0.25)")]),
        ("P4", 120, 115, 0, [("1", "thru_hole", "oval", (1.6, 1.6), "A", THT, "0.8", "(thermal_bridge_angle 45)")]),
        ("P5", 125, 115, 60, [("1", "thru_hole", "oval", (2.6, 1.4), "A", THT, "oval 1.6 0.7", "")]),
        ("P6", 130, 115, 0, [("1", "smd", "rect", (1.6, 1.6), "A", SMD, None, "(zone_connect 0)")]),
        ("P7", 135, 115, 0, [("1", "smd", "rect", (1.6, 1.6), "A", SMD, None, "(zone_connect 2)")]),
        ("P8", 105, 122, 0, [("1", "np_thru_hole", "circle", (1.2, 1.2), None, THT, "1.2", "")]),
        ("P9", 110, 122, 0, [("1", "thru_hole", "rect", (1.8, 1.8), "B", THT, "0.9", "")]),
        ("P10", 116, 122, 0, [("1", "smd", "circle", (1.4, 1.4), "A", ("B.Cu", "B.Mask"), None, "(zone_connect 1)")]),
        ("P11", 122, 122, 0, [("1", "smd", "rect", (0.2, 3), "A", SMD, None, "")]),  # spoke width > pad
        ("P12", 128, 122, 0, [("1", "smd", "trapezoid", (2, 1.6), "A", SMD, None, "(rect_delta 0 0.5) (thermal_gap 0.25) (thermal_bridge_width 0.3)")]),
        ("P13", 134, 122, 0, [("1", "smd", "roundrect", (2, 1.6), "A", SMD, None,
                               "(roundrect_rratio 0.2) (chamfer_ratio 0.25) (chamfer top_left bottom_right)")]),
    ]
    for ref, x, y, rot, pl in pads:
        b.footprint(ref, x, y, [b.pad(n, k, s, (0, 0), sz, net, ls, rot=rot, drill=d, extra=e)
                                for n, k, s, sz, net, ls, d, e in pl], rot=rot,
                    layer="B.Cu" if pl[0][5][0] == "B.Cu" else "F.Cu")
    return b


def hatch_noncu():
    """fillNonCopperZone: hatch pattern, keepout, knockout text, min thickness 0."""
    b = Board("hatch_noncu")
    hf = ("(mode hatch) (hatch_thickness 0.5) (hatch_gap 1.5) (hatch_orientation -20) "
          "(hatch_smoothing_level 1) (hatch_smoothing_value 0.5) (hatch_border_algorithm hatch_thickness) "
          "(hatch_min_hole_area 0.2)")
    b.zone(box(101, 101, 119, 129), "F.SilkS", name="silk_hatch", fill=hf, min_thickness=0.2,
           extra='(property (layer "F.SilkS") (hatch_position (xy 0.5 0.25))) ')
    b.zone(box(121, 101, 139, 114), "Dwgs.User", name="solid_zero", min_thickness=0)
    b.zone(box(121, 116, 139, 129), ("F.Fab", "B.Fab"), name="fab", min_thickness=0.3,
           fill="(smoothing fillet) (radius 1.5)")
    b.zone(box(105, 105, 112, 110), "F.SilkS", name="ko", keepout="not_allowed")
    b.zone(box(125, 120, 130, 135), ("F.Fab", "F.Cu"), name="ko2", keepout="not_allowed")
    b.zone(box(132, 104, 136, 108), "Dwgs.User", name="ko_allowed", keepout="allowed")
    b.text("KO", 110, 120, "F.SilkS", knockout=True, size=3)
    b.text("NK", 130, 107, "Dwgs.User", knockout=False, size=2)
    b.text("FB", 130, 123, "F.Fab", knockout=True, size=2, rot=30)
    b.line(102, 125, 118, 127, "F.SilkS", 0.3)
    return b


def islands():
    """Island removal modes with the area threshold, stale flags, unconnected pours, net 0."""
    b = Board("islands")
    # Z1: AREA mode, 3 mm^2 threshold; tracks of net B cut it into a connected part, a small and
    # a large island
    b.zone(box(101, 101, 119, 114), "F.Cu", net="A", name="area",
           fill="(island_removal_mode 2) (island_area_min 3)", connect="yes")
    b.footprint("P1", 104, 104, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), "A", SMD)])
    b.seg(100, 108, 120, 108, "F.Cu", "B")       # splits off the lower half (large island)
    b.seg(110, 108, 110, 115, "F.Cu", "B")       # splits the lower half in two
    b.seg(116, 100, 116, 108, "F.Cu", "B")       # upper right piece
    b.seg(116, 106, 120, 106, "F.Cu", "B")       # small island (about 3 x 1.2 mm)
    b.seg(116, 103, 120, 103, "F.Cu", "B")
    # Z2: NEVER mode on the back side, same cuts
    b.zone(box(101, 101, 119, 114), "B.Cu", net="A", name="never", fill="(island_removal_mode 1)", connect="yes")
    b.footprint("P2", 104, 104, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), "A", ("B.Cu", "B.Mask"))], layer="B.Cu")
    b.seg(100, 108, 120, 108, "B.Cu", "B")
    b.seg(110, 108, 110, 115, "B.Cu", "B")
    # Z3: ALWAYS mode
    b.zone(box(121, 101, 139, 114), "F.Cu", net="B", name="always", fill="(island_removal_mode 0)", connect="yes")
    b.footprint("P3", 124, 104, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), "B", SMD)])
    b.seg(120, 108, 140, 108, "F.Cu", "A")
    b.seg(130, 100, 130, 108, "F.Cu", "A")
    # Z4: a pour of a net without any pad (fully isolated on every layer: preserved), cut in two
    b.zone(box(101, 116, 119, 129), ("F.Cu", "B.Cu"), net="C", name="unconnected", fill="(island_removal_mode 0)")
    b.seg(110, 115, 110, 130, "F.Cu", "B")
    # Z5: net-less zone
    b.zone(box(121, 116, 139, 129), "F.Cu", net="", name="nonet", fill="(island_removal_mode 2) (island_area_min 3)")
    b.seg(130, 115, 130, 130, "F.Cu", "B")
    b.footprint("P5", 124, 120, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), "A", SMD)])
    b.footprint("P6", 135, 120, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), None, SMD)])
    # Z6: connected on F.Cu only, the B.Cu layer is fully isolated (kept), F.Cu has an island
    b.zone(box(121, 101, 139, 114), "B.Cu", net="B", name="back_iso", fill="(island_removal_mode 0)")
    return b


def fp_zones():
    """Zones owned by footprints: filled-zone knockouts (never filled by FillAllZones), keepouts."""
    b = Board("fp_zones")
    b.zone(FULL, "F.Cu", net="A", name="board", priority=0)
    z_other = b.zone_str(box(-3, -2, 3, 2), "F.Cu", net="B", name="fp_other", priority=2)
    z_same = b.zone_str(box(-3, -2, 3, 2), "F.Cu", net="A", name="fp_same", priority=3)
    z_ko = b.zone_str(box(-3, -2, 3, 2), "F.Cu", name="fp_ko", keepout="not_allowed")
    z_ko_arc = b.zone_str(None, ("F.Cu", "B.Cu"), name="fp_ko_arc", keepout="not_allowed",
                          raw_pts="(xy -3 -2) (xy 3 -2) (arc (start 3 -2) (mid 5 0) (end 3 2)) (xy -3 2)")
    b.footprint("F1", 108, 108, [b.pad("1", "smd", "rect", (-4.5, 0), (1, 1), "B", SMD, rot=90), z_other], rot=90)
    b.footprint("F2", 120, 108, [b.pad("1", "smd", "rect", (-4.5, 0), (1, 1), "A", SMD, rot=45), z_same], rot=45)
    b.footprint("F3", 132, 108, [z_ko], rot=30)
    b.footprint("F4", 110, 122, [z_ko_arc], rot=200)
    b.footprint("F5", 125, 122, [b.pad("1", "smd", "rect", (0, 0), (2, 2), "A", ("B.Cu", "B.Mask"), rot=180), z_ko],
                rot=180, layer="B.Cu")
    b.zone(FULL, "B.Cu", net="A", name="board_back", priority=0)
    return b


def mixed_layers():
    """A zone on copper and non-copper layers (fillCopperZone on a technical layer), Margin
    graphics, an edge cut-out."""
    b = Board("mixed_layers", layers=4)
    b.zone(FULL, ("F.Cu", "In1.Cu", "F.SilkS", "F.Mask"), net="A", name="mixed", fill="(island_removal_mode 1)")
    b.line(105, 101, 105, 129, "Margin", 0.15)
    b.line(100, 126, 140, 124, "Margin", 0.6)
    b.circle(130, 110, 3, "Edge.Cuts", 0.05)
    b.line(108, 104, 125, 104, "F.SilkS", 0.3)
    b.rect(110, 108, 114, 112, "F.SilkS", 0.2)
    b.rect(118, 108, 122, 112, "F.Mask", 0.2, fill="yes")
    b.text("SILK", 115, 118, "F.SilkS", size=2)
    b.rect(108, 120, 112, 123, "F.Cu", 0.2, fill="yes", net="A")
    b.rect(114, 120, 118, 123, "F.Cu", 0.2, fill="yes", net="B")
    b.circle(122, 121.5, 1.5, "In1.Cu", 0.3)
    b.seg(107, 115, 125, 115, "In1.Cu", "B")
    b.via(125, 115, "B")
    b.via(110, 126, "A")
    b.footprint("P1", 134, 118, [b.pad("1", "thru_hole", "circle", (0, 0), (1.8, 1.8), "A", THT, drill=0.9),
                                 b.pad("2", "thru_hole", "circle", (0, 4), (1.8, 1.8), "B", THT, drill=0.9),
                                 b.pad("3", "smd", "rect", (-4, 4), (1.5, 1.5), "A", ("F.Cu", "F.Mask", "F.Paste"))],
                attr="through_hole")
    b.footprint("P2", 134, 104, [b.pad("1", "np_thru_hole", "circle", (0, 0), (1.5, 1.5), None, ("F&B.Cu", "*.Mask"), drill=1.5),
                                 b.pad("2", "np_thru_hole", "circle", (-4, 0), (1.5, 1.5), None, ("*.Mask",), drill=1.5)],
                attr="through_hole")
    return b


def rules_conn():
    """Zone connection, thermal and clearance constraints through custom rules, with the pad /
    footprint / zone override order of EvalRules and EvalZoneConnection."""
    b = Board("rules_conn")
    b.zone(box(101, 101, 139, 114), "F.Cu", net="A", name="thermal_zone")
    b.zone(box(101, 116, 139, 129), "F.Cu", net="A", name="tht_zone", connect="thru_hole_only")
    b.zone(FULL, "B.Cu", net="A", name="solid_zone", connect="yes")
    def fp(ref, x, y, pads, **k):
        b.footprint(ref, x, y, [b.pad(n, kind, shape, at, size, net, ls, drill=d, extra=e)
                                for n, kind, shape, at, size, net, ls, d, e in pads], **k)
    for row, y in (("a", 107), ("b", 122)):
        fp(f"R1{row}", 105, y, [("1", "smd", "rect", (0, 0), (1.6, 1.6), "A", SMD, None, "")])           # rule: footprint solid
        fp(f"R2{row}", 110, y, [("2", "smd", "rect", (0, 0), (1.6, 1.6), "A", SMD, None, "")])           # rule: pad none
        fp(f"R3{row}", 115, y, [("3", "thru_hole", "circle", (0, 0), (1.8, 1.8), "A", THT, 0.9, "")],    # rule: gap + spoke
           attr="through_hole")
        fp(f"R4{row}", 120, y, [("4", "smd", "rect", (0, 0), (1.6, 1.6), "A", SMD, None, "(zone_connect 2)"),
                                ("5", "smd", "rect", (0, 2.5), (1.6, 1.6), "A", SMD, None, "")],
           zone_connect=0)                                                                                # fp none, pad solid
        fp(f"R5{row}", 125, y, [("6", "thru_hole", "circle", (0, 0), (1.8, 1.8), "A", THT, 0.9, "(zone_connect 3)"),
                                ("7", "smd", "rect", (0, 2.5), (1.6, 1.6), "A", SMD, None, "(zone_connect 3)")],
           attr="through_hole")                                                                           # THT thermal
        fp(f"R6{row}", 130, y, [("8", "thru_hole", "circle", (0, 0), (1.8, 1.8), "A", THT, 0.9,
                                 "(thermal_bridge_width 0.9) (thermal_gap 0.2) (thermal_bridge_angle 30)"),
                                ("9", "thru_hole", "rect", (0, 2.7), (1.8, 1.8), "B", THT, 0.9, "(clearance 0.6)")],
           attr="through_hole")
        fp(f"R7{row}", 135, y, [("10", "np_thru_hole", "circle", (0, 0), (1.2, 1.2), None, THT, 1.2, ""),
                                ("11", "smd", "rect", (0, 2.5), (1.6, 1.6), "A", SMD, None, "")], zone_connect=1)
    b.seg(102, 103, 138, 103, "F.Cu", "B")
    b.via(137, 112, "B")
    b.via(103, 112, "A")
    b.side["kicad_dru"] = '''(version 1)
(rule "fp solid" (condition "A.Reference == 'R1a' || A.Reference == 'R1b'") (constraint zone_connection solid))
(rule "pad none" (condition "A.Type == 'Pad' && A.Pad_Number == '2'") (constraint zone_connection none))
(rule "gap" (condition "A.Pad_Number == '3'") (constraint thermal_relief_gap (min 0.8mm)))
(rule "spoke" (condition "A.Pad_Number == '3'") (constraint thermal_spoke_width (opt 0.9mm)))
(rule "spoke clamp" (condition "A.Pad_Number == '11'") (constraint thermal_spoke_width (min 0.7mm) (opt 0.3mm) (max 1mm)))
(rule "phys" (condition "A.Pad_Number == '9' || B.Pad_Number == '9'") (constraint physical_clearance (min 0.75mm)))
(rule "phys hole" (constraint physical_hole_clearance (min 0.55mm)))
(rule "hole" (layer "B.Cu") (constraint hole_clearance (min 0.65mm)))
(rule "edge" (constraint edge_clearance (min 0.9mm)))
(rule "cl" (layer "F.Cu") (condition "A.NetName == 'B' && B.Type == 'Zone'") (constraint clearance (min 0.45mm)))
'''
    return b


def backdrill():
    """Backdrilled and post-machined pads and vias (solid and hatch zones)."""
    b = Board("backdrill", layers=4)
    b.zone(FULL, ("F.Cu", "In1.Cu", "In2.Cu", "B.Cu"), net="A", name="solid")
    bd = '(backdrill (size 1.3) (layers "B.Cu" "In2.Cu"))'
    td = '(tertiary_drill (size 1.4) (layers "F.Cu" "In1.Cu"))'
    cb = "(front_post_machining counterbore (size 2.4) (depth 0.5))"
    cs = "(back_post_machining countersink (size 2.6) (depth 0.4) (angle 90))"
    specs = [("1", "A", bd), ("2", "B", bd), ("3", "A", td), ("4", "A", cb), ("5", "B", cs), ("6", "A", bd + " " + td),
             ("7", "A", cs)]
    for i, (num, net, extra) in enumerate(specs):
        b.footprint(f"J{num}", 105 + 5 * i, 106, [b.pad(num, "thru_hole", "circle", (0, 0), (1.9, 1.9), net, THT,
                                                         drill=f"0.9) {extra[:-1]}")], attr="through_hole")
    vb = '(backdrill (size 0.5) (layers "B.Cu" "In2.Cu"))'
    vt = '(tertiary_drill (size 0.5) (layers "F.Cu" "In1.Cu"))'
    for i, (net, extra) in enumerate((("A", vb), ("B", vb), ("A", vt), ("B", vt), ("A", vb + " " + vt),
                                      ("A", "(front_post_machining counterbore (size 1.1) (depth 0.4))"))):
        b.via(105 + 5 * i, 113, net, extra=extra)
    # a hatch zone above the solid one on B.Cu
    hf = ("(mode hatch) (hatch_thickness 0.4) (hatch_gap 1.4) (hatch_orientation 0) (hatch_smoothing_level 0) "
          "(hatch_smoothing_value 0.1) (hatch_border_algorithm hatch_thickness) (hatch_min_hole_area 0.3)")
    b.zone(box(101, 117, 139, 129), ("In2.Cu", "B.Cu"), net="A", name="hatch", priority=1, fill=hf, clearance=0.2)
    for i, (net, extra) in enumerate((("A", vb), ("B", vb), ("A", vt), ("A", ""), ("A", vb + " " + vt))):
        b.via(105 + 6 * i, 123, net, extra=extra)
    return b


def arcs_smooth(name, external_fillets):
    """Arc outlines, chamfer / fillet smoothing, out-of-board islands."""
    b = Board(name, edge=False)
    # board outline with two rounded corners
    b.line(102, 100, 138, 100, "Edge.Cuts", 0.05)
    b.arc(138, 100, 139.414214, 100.585786, 140, 102, "Edge.Cuts", 0.05)
    b.line(140, 102, 140, 130, "Edge.Cuts", 0.05)
    b.line(140, 130, 100, 130, "Edge.Cuts", 0.05)
    b.line(100, 130, 100, 102, "Edge.Cuts", 0.05)
    b.arc(100, 102, 100.585786, 100.585786, 102, 100, "Edge.Cuts", 0.05)
    arc_pts = ("(xy 96 98) (xy 118 98) (xy 118 112) (arc (start 118 112) (mid 113 117) (end 108 112)) "
               "(xy 104 112) (xy 104 122) (xy 96 122)")
    b.zone(None, "F.Cu", net="A", name="arc_fillet", raw_pts=arc_pts, fill="(smoothing fillet) (radius 1.2)")
    b.zone(box(120, 96, 146, 112), "F.Cu", net="B", name="chamfer", fill="(smoothing chamfer) (radius 1.5)")
    b.zone(box(106, 114, 144, 134), "F.Cu", net="A", name="fillet_low", fill="(smoothing fillet) (radius 2)",
           priority=0)
    b.zone(box(118, 118, 132, 126), "F.Cu", net="B", name="inner_fillet", fill="(smoothing fillet) (radius 3)",
           priority=1)
    b.zone(None, "F.Cu", name="ko_arc", keepout="not_allowed", fill="(smoothing fillet) (radius 0.5)",
           raw_pts="(xy 108 102) (xy 114 102) (arc (start 114 102) (mid 116 104) (end 114 106)) (xy 108 106)")
    b.zone(box(126, 100, 132, 104), "F.Cu", name="ko_chamfer", keepout="not_allowed",
           fill="(smoothing chamfer) (radius 1)")
    # a zone mostly outside the board: its fill outside the edge is removed
    b.zone(box(134, 106, 150, 116), "B.Cu", net="A", name="outside", fill="(smoothing fillet) (radius 4)")
    b.zone(box(96, 96, 112, 108), "B.Cu", net="B", name="corner", fill="(smoothing chamfer) (radius 6)")
    b.footprint("P1", 100.8, 110, [b.pad("1", "smd", "rect", (0, 0), (1, 1), "A", SMD)])
    b.footprint("P2", 136, 102, [b.pad("1", "smd", "rect", (0, 0), (1, 1), "B", SMD)])
    b.footprint("P3", 137, 111, [b.pad("1", "thru_hole", "circle", (0, 0), (1.6, 1.6), "A", THT, drill=0.8)],
                attr="through_hole")
    if external_fillets:
        b.side["kicad_pro"] = ('{\n  "board": {\n    "design_settings": {\n      "zones_allow_external_fillets": true\n'
                               '    }\n  },\n  "meta": {\n    "filename": "%s.kicad_pro",\n    "version": 3\n  }\n}\n' % name)
    return b


def priority():
    """Priorities, the UUID tie-break, same-net overlap, degenerate zones, refill reclaim."""
    b = Board("priority")
    # equal priority, different nets: HigherPriority() decides by UUID
    b.zone(box(101, 101, 118, 114), "F.Cu", net="A", name="tie_low", uuid="11111111-0000-4000-8000-000000000001")
    b.zone(box(110, 105, 126, 112), "F.Cu", net="B", name="tie_high", uuid="99999999-0000-4000-8000-000000000002")
    b.zone(box(104, 108, 114, 118), "F.Cu", net="C", name="tie_mid", uuid="55555555-0000-4000-8000-000000000003")
    # same net, different priorities (the lower one loses the higher one's outline, issue 23790)
    b.zone(box(121, 101, 139, 129), "F.Cu", net="A", name="same_low", priority=1)
    b.zone(box(126, 114, 136, 124), "F.Cu", net="A", name="same_high", priority=4, fill="(island_removal_mode 0)",
           min_thickness=0.5, clearance=0.6)
    # a different-net zone of the highest priority whose only piece is an island (ALWAYS): its
    # removal frees the area for the zones below (iterative refill)
    b.zone(box(123, 104, 137, 110), "F.Cu", net="D", name="island_top", priority=6, fill="(island_removal_mode 0)")
    b.zone(box(101, 116, 119, 129), "F.Cu", net="B", name="chain0", priority=0)
    b.zone(box(103, 118, 117, 127), "F.Cu", net="C", name="chain1", priority=1, fill="(island_removal_mode 0)")
    b.zone(box(105, 120, 115, 125), "F.Cu", net="D", name="chain2", priority=2, fill="(island_removal_mode 0)")
    # degenerate zones
    b.zone([(102, 102), (106, 106)], "F.Cu", net="A", name="two_corners", priority=9)
    b.footprint("P1", 103, 103, [b.pad("1", "smd", "rect", (0, 0), (1, 1), "A", SMD)])
    b.footprint("P2", 124, 108.5, [b.pad("1", "smd", "rect", (0, 0), (1, 1), "B", SMD)])
    b.footprint("P3", 106, 116.5, [b.pad("1", "smd", "rect", (0, 0), (1, 1), "C", SMD)])
    b.footprint("P4", 122.5, 127, [b.pad("1", "smd", "rect", (0, 0), (1, 1), "A", SMD)])
    b.footprint("P5", 102.2, 122, [b.pad("1", "smd", "rect", (0, 0), (0.8, 0.8), "B", SMD)])
    b.footprint("P6", 104.2, 122, [b.pad("1", "smd", "rect", (0, 0), (0.8, 0.8), "C", SMD)])
    return b


def thin():
    """Zones too thin for the min-width cycle (half_min_width - epsilon <= epsilon)."""
    b = Board("thin")
    b.zone(box(101, 101, 119, 129), "F.Cu", net="A", name="thin", min_thickness=0.003)
    b.zone(box(121, 101, 139, 129), "F.Cu", net="A", name="edge", min_thickness=0.0041)
    b.zone(box(101, 101, 139, 129), "B.Cu", net="B", name="zero", min_thickness=0, priority=1)
    b.zone(box(105, 105, 135, 125), "B.Cu", net="A", name="under", min_thickness=0.003, priority=0)
    b.footprint("P1", 106, 108, [b.pad("1", "thru_hole", "circle", (0, 0), (1.8, 1.8), "A", THT, drill=0.9),
                                 b.pad("2", "thru_hole", "circle", (0, 6), (1.8, 1.8), "B", THT, drill=0.9)],
                attr="through_hole")
    b.footprint("P2", 128, 108, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), "A", SMD),
                                 b.pad("2", "smd", "rect", (0, 6), (1.5, 1.5), "B", SMD)])
    b.seg(102, 120, 138, 121, "F.Cu", "B", width=0.2)
    return b


def prototype():
    """m_PrototypeZoneFill of the project local settings: a hatch zone is filled solid."""
    b = Board("prototype")
    hf = ("(mode hatch) (hatch_thickness 0.5) (hatch_gap 1.5) (hatch_orientation 0) (hatch_smoothing_level 0) "
          "(hatch_smoothing_value 0.1) (hatch_border_algorithm hatch_thickness) (hatch_min_hole_area 0.3)")
    b.zone(FULL, "F.Cu", net="A", name="proto", fill=hf)
    b.footprint("P1", 110, 110, [b.pad("1", "thru_hole", "circle", (0, 0), (1.8, 1.8), "A", THT, drill=0.9)],
                attr="through_hole")
    b.via(120, 110, "A")
    b.via(125, 112, "B")
    b.side["kicad_prl"] = ('{\n  "board": {\n    "prototype_zone_fills": true\n  },\n  "meta": {\n'
                           '    "filename": "prototype.kicad_prl",\n    "version": 5\n  }\n}\n')
    b.side["kicad_pro"] = '{\n  "meta": {\n    "filename": "prototype.kicad_pro",\n    "version": 3\n  }\n}\n'
    return b


def mixed_isolated():
    """F-1: the reference segfaults (double deletion of outline 0); no golden."""
    b = Board("mixed_isolated")
    b.zone(FULL, ("F.Cu", "F.SilkS"), net="A", name="mixed")
    return b


def dup_uuid():
    """F-2: the reference hangs (dependency cycle of the fill scheduler); no golden."""
    b = Board("dup_uuid")
    u = "aaaaaaaa-0000-4000-8000-000000000001"
    b.zone(box(101, 101, 125, 120), "F.Cu", net="A", name="za", uuid=u)
    b.zone(box(115, 110, 139, 129), "F.Cu", net="B", name="zb", uuid=u)
    b.footprint("P1", 104, 104, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), "A", SMD)])
    b.footprint("P2", 136, 126, [b.pad("1", "smd", "rect", (0, 0), (1.5, 1.5), "B", SMD)])
    return b


# The boards the reference cannot refill (no golden; make_rev_zf_goldens.sh skips them).
NO_REFERENCE = ("mixed_isolated", "dup_uuid")


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for b in (custom_spokes(), hatch_vias(), hatch_noncu(), islands(), fp_zones(), mixed_layers(), rules_conn(),
              backdrill(), arcs_smooth("arcs_smooth", False), arcs_smooth("arcs_extfillet", True), priority(),
              thin(), prototype(), mixed_isolated(), dup_uuid()):
        b.write(out)


if __name__ == "__main__":
    main()
