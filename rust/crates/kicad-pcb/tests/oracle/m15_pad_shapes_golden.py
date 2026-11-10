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
# Package M15 (pad shapes): regenerates tests/golden/m15_pad_shapes.txt from the reference
# pcb-dump (rust/docs/ORACLE.md) on scratch copies of tests/oracle/m15_pad_shapes.kicad_pcb and
# a few qa/data/pcbnew boards. Lines are `<board>|SECTION|KEY|FIELD=VALUE`: the members the
# Rust replay (`pad::shapes::tests::oracle_pad_shapes`) rebuilds each pad from, unhashed, and
# the shape results (effective shapes, polygons, hole shapes, TransformShapeToPolygon /
# TransformHoleToPolygon, bounding boxes and radius) as FNV-1a hashes (`--hash`). `proxy` lines
# name the custom primitives that are `gr_bbox` proxies (the dump does not print the flag).
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
QA = os.path.normpath(os.path.join(HERE, "../../../../../qa/data/pcbnew"))
PCB_DUMP = "/home/user/kicad-oracle/bin/pcb-dump"
BOARDS = [
    ("m15", os.path.join(HERE, "m15_pad_shapes.kicad_pcb")),
    ("padstacks_complex", os.path.join(QA, "padstacks_complex.kicad_pcb")),
    ("padstacks", os.path.join(QA, "padstacks.kicad_pcb")),
    ("custom_pads", os.path.join(QA, "custom_pads.kicad_pcb")),
    ("issue4139", os.path.join(QA, "issue4139.kicad_pcb")),
    ("issue6443", os.path.join(QA, "issue6443.kicad_pcb")),
]

MEMBERS = re.compile(
    r"^(board\|board\|(ds\.max_error|ds\.solder_mask_expansion|copper_layer_count)="
    r"|footprint\|[^|]*\|(transform\.(translate|rotate|scale)|layer|flipped|local_solder_mask_margin)="
    r"|item\|[^|]*\|(parent|layer|net)="
    r"|pad\|[^|]*\|(attribute|library_position|fp_relative_orientation|unconnected_layer_mode"
    r"|solder_mask_expansion\.[^=]*|on_layer\.[^=]*|flash\.[^=]*"
    r"|padstack\.(mode|layers|drill\.(size|shape|start|end)|(secondary|tertiary)_drill\.size"
    r"|(front|back)_post_machining\.(mode|size)|(front|back)_outer\.solder_mask_margin"
    r"|copper\.[^.]+\.[^.]+\.(shape|anchor_shape|size|offset|round_rect_radius_ratio"
    r"|chamfered_rect_ratio|chamfered_rect_positions|trapezoid_delta_size"
    r"|primitive\.[0-9]+\.[a-z_]+)))=)")
RESULTS = re.compile(
    r"^(pad\|[^|]*\|(shape|shape_flashed|effective_polygon|effective_polygon_out|hole_poly|tpoly)\."
    r"|pad\|[^|]*\|hole_shape="
    r"|bbox\|[^|]*\|(bbox(\.[^=]*)?|bounding_radius)=)")


def proxies(text):
    """(pad uuid, primitive index) of every gr_bbox among a pad's top-level primitives."""
    out = []

    for m in re.finditer(r"\(pad\s", text):
        depth, i = 0, m.start()

        while True:
            c = text[i]
            depth += c == "("
            depth -= c == ")"
            i += 1

            if depth == 0:
                break

        block = text[m.start():i]
        uuid = re.search(r"\((?:uuid|tstamp)\s+\"?([0-9a-fA-F-]+)", block)
        prims = re.search(r"\(primitives(.*)", block, re.S)

        if not uuid or not prims or "(padstack" in block:
            continue

        kinds = re.findall(r"\((gr_[a-z]+)", prims.group(1))

        for n, kind in enumerate(kinds):
            if kind == "gr_bbox":
                out.append((uuid.group(1).lower(), n))

    return out


def main():
    golden = os.path.join(HERE, "../golden/m15_pad_shapes.txt")
    scratch = tempfile.mkdtemp()
    lines = []

    try:
        env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(scratch, "home"))

        for tag, path in BOARDS:
            copy = os.path.join(scratch, os.path.basename(path))
            shutil.copy(path, copy)
            args = [PCB_DUMP, "board", "--sections", "board,item,footprint,pad,bbox"]
            plain = subprocess.run(args + [copy], env=env, capture_output=True, text=True).stdout
            hashed = subprocess.run(args + ["--hash", copy], env=env, capture_output=True,
                                    text=True).stdout
            pads = {l.split("|")[1] for l in plain.splitlines() if l.startswith("pad|")}

            for l in plain.splitlines():
                if MEMBERS.match(l):
                    section, key = l.split("|")[:2]

                    if section in ("item", "bbox") and key not in pads:
                        continue

                    lines.append(tag + "|" + l)

            for l in hashed.splitlines():
                if RESULTS.match(l) and l.split("|")[1] in pads:
                    lines.append(tag + "|" + l)

            with open(path, encoding="utf-8") as f:
                for uuid, n in proxies(f.read()):
                    lines.append(f"{tag}|proxy|{uuid}|primitive.{n}=true")
    finally:
        shutil.rmtree(scratch)

    with open(golden, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    sys.exit(main())
