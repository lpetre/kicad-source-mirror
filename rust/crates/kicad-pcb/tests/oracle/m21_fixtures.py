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
#
# Package M21: writes the malformed / unusual board outline fixtures m21_fixtures/*.kicad_pcb
# (footprint-free boards, so the Rust parser loads them today). m21_outline_golden.sh runs the
# oracle on them together with the corpus.
import os

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "m21_fixtures")

HEADER = """(kicad_pcb
\t(version 20260410)
\t(generator "pcbnew")
\t(generator_version "10.99")
\t(general (thickness 1.6) (legacy_teardrops no))
\t(paper "A4")
\t(layers
\t\t(0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user "F.Silkscreen")
\t\t(25 "Edge.Cuts" user) (31 "F.CrtYd" user "F.Courtyard")
\t)
\t(setup (pad_to_mask_clearance 0))
\t(net 0 "")
"""

counter = [0]


def uuid():
    counter[0] += 1
    return "21000000-0000-0000-0000-%012d" % counter[0]


def tail(layer="Edge.Cuts", width=0.05):
    return f'(stroke (width {width}) (type solid)) (layer "{layer}") (uuid "{uuid()}")'


def line(x1, y1, x2, y2, **kw):
    return f"(gr_line (start {x1} {y1}) (end {x2} {y2}) {tail(**kw)})"


def arc(x1, y1, xm, ym, x2, y2, **kw):
    return f"(gr_arc (start {x1} {y1}) (mid {xm} {ym}) (end {x2} {y2}) {tail(**kw)})"


def circle(cx, cy, r, **kw):
    return f"(gr_circle (center {cx} {cy}) (end {cx + r} {cy}) {tail(**kw)} (fill no))"


def rect(x1, y1, x2, y2, radius=None, **kw):
    rad = f" (radius {radius})" if radius else ""
    return f"(gr_rect (start {x1} {y1}) (end {x2} {y2}){rad} {tail(**kw)} (fill no))"


def poly(pts, **kw):
    xy = " ".join(f"(xy {x} {y})" for x, y in pts)
    return f"(gr_poly (pts {xy}) {tail(**kw)} (fill no))"


def curve(p0, c1, c2, p1, **kw):
    xy = " ".join(f"(xy {x} {y})" for x, y in (p0, c1, c2, p1))
    return f"(gr_curve (pts {xy}) {tail(**kw)})"


def ellipse(cx, cy, major, minor, rot, **kw):
    return (f"(gr_ellipse (center {cx} {cy}) (major_radius {major}) (minor_radius {minor}) "
            f"(rotation_angle {rot}) {tail(**kw)} (fill no))")


def ellipse_arc(cx, cy, major, minor, rot, start, end, **kw):
    return (f"(gr_ellipse_arc (center {cx} {cy}) (major_radius {major}) (minor_radius {minor}) "
            f"(rotation_angle {rot}) (start_angle {start}) (end_angle {end}) {tail(**kw)})")


def box(x1, y1, x2, y2):
    return [line(x1, y1, x2, y1), line(x2, y1, x2, y2), line(x2, y2, x1, y2), line(x1, y2, x1, y1)]


BOARDS = {
    # open outline: one gap of 0.5 mm, reported from both chain ends
    "gap": [line(100, 100, 130, 100), line(130, 100, 130, 120), line(130, 120, 100, 120),
            line(100, 120, 100, 100.5)],
    # gaps below the chaining epsilon (closure by SetPoint) and an arc closing the contour
    "near_closed": [line(100, 100, 130, 100.004), line(130, 100.004, 130, 120),
                    line(130.003, 120, 110, 120), arc(110, 120, 102.9289, 117.0711, 100, 110.006),
                    line(100, 110, 100.002, 100.002)],
    # outline of lines and arcs, holes of every closed kind, an island in a hole
    "nested": [line(105, 100, 145, 100), arc(145, 100, 148.5355, 101.4645, 150, 105),
               line(150, 105, 150, 135), arc(150, 135, 148.5355, 138.5355, 145, 140),
               line(145, 140, 105, 140), arc(105, 140, 101.4645, 138.5355, 100, 135),
               line(100, 135, 100, 105), arc(100, 105, 101.4645, 101.4645, 105, 100),
               circle(110, 110, 3), rect(120, 105, 128, 112, radius=1.5),
               poly([(132, 106), (140, 106), (136, 113)]), ellipse(112, 128, 6, 3, 30),
               rect(122, 118, 146, 136), rect(126, 122, 140, 132), circle(133, 127, 2)],
    # crossing segments (bow tie)
    "bowtie": [line(100, 100, 130, 120), line(130, 120, 130, 100), line(130, 100, 100, 120),
               line(100, 120, 100, 100)],
    # a T junction: the chain meets an already chained shape
    "branch": box(100, 100, 130, 120) + [line(115, 100, 115, 90)],
    # overlapping closed shapes inside an outline (crossing contours, test_items overlaps)
    "overlap": box(100, 100, 140, 130) + [circle(115, 115, 5), circle(121, 115, 5),
                                          rect(125, 105, 135, 112), rect(130, 108, 138, 116),
                                          circle(140, 115, 3)],
    # degenerate items
    "small": box(100, 100, 130, 120) + [line(110, 110, 110, 110), circle(115, 110, 0.0005),
                                        rect(120, 110, 120.0005, 110.0005),
                                        arc(125, 110, 125.0001, 110.0001, 125.0002, 110),
                                        ellipse(105, 105, 2, 0.0005, 0)],
    # a bezier-bounded slot crossing a circular hole and the outline, an ellipse arc edge
    "bezier_slot": [line(100, 100, 140, 100), line(140, 100, 140, 130),
                    curve((140, 130), (130, 135), (110, 125), (100, 130)), line(100, 130, 100, 100),
                    curve((110, 110), (115, 105), (120, 115), (125, 110)),
                    line(125, 110, 125, 114), curve((125, 114), (120, 119), (115, 109), (110, 114)),
                    line(110, 114, 110, 110), circle(118, 112, 3),
                    curve((130, 105), (132, 103), (134, 107), (136, 105)), line(136, 105, 136, 108),
                    line(136, 108, 130, 108), line(130, 108, 130, 105)],
    "ellipse": [ellipse(120, 115, 20, 12, 15), ellipse_arc(120, 115, 8, 5, 0, 0, 180),
                line(128, 115, 112, 115)],
    # two disjoint outlines, a duplicated segment, a polygon outline with a polygon hole
    "disjoint": box(100, 100, 120, 120) + box(130, 100, 150, 120) + [line(130, 100, 150, 100)]
                + [poly([(160, 100), (190, 100), (190, 130), (160, 130)]),
                   poly([(170, 110), (180, 110), (175, 120)])],
    # no edges at all: inferred outline from the other items, footprint fallback
    "no_edges": [line(100, 100, 120, 110, layer="F.SilkS")],
    "empty": [],
    # courtyard-like shapes on Edge.Cuts: two arcs forming a circle, a single segment
    "two_arcs": [arc(110, 100, 120, 110, 110, 120), arc(110, 120, 100, 110, 110, 100),
                 line(130, 100, 140, 100)],
}


def main():
    os.makedirs(OUT, exist_ok=True)

    for name, items in BOARDS.items():
        text = HEADER + "".join(f"\t{it}\n" for it in items) + "\t(embedded_fonts no)\n)\n"

        with open(os.path.join(OUT, f"m21_{name}.kicad_pcb"), "w") as f:
            f.write(text)


if __name__ == "__main__":
    main()
