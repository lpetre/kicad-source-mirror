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

"""DRC-LP: crafted library parity cases and their reference golden.

    python3 -I rust/crates/kicad-session/tests/oracle/gen_drc_lp_cases.py data
    python3 -I rust/crates/kicad-session/tests/oracle/gen_drc_lp_cases.py golden

`data` writes tests/data/drc_lp/ from demos/ecc83 (board stripped to its footprints, the
project library, crafted libraries and tables): one board footprint per case, each a copy of
a matching footprint with one difference (pads, graphics, attributes, net ties, texts, 3D
model, overrides, orientation, side), or one library-table situation (not in the table,
disabled row, missing directory, a corrupt file in the library, unknown plugin type, nested
table, `$(KIPRJMOD)`, relative URI, global-table precedence, missing footprint).

`golden` runs the reference kicad-cli (`pcb drc --format json --severity-all`) on a copy of
the project, with the global table of tests/data/drc_lp/global in a private config home,
and writes tests/golden/drc_lp_library_parity.json: the `lib_footprint_*` entries in report
order. The Rust replay is tests/drc_lp_library_parity.rs.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
DATA = os.path.join(HERE, "..", "data", "drc_lp")
GOLDEN = os.path.join(HERE, "..", "golden", "drc_lp_library_parity.json")
DEMO = os.path.join(KICAD, "demos", "ecc83")
REF_CLI = "/home/user/kicad-oracle/bin/kicad-cli"
BOARD = "drc_lp.kicad_pcb"


def blocks(text):
    """Top-level children of the root list: (start, end) spans."""
    out = []
    depth = 0
    in_str = False
    start = None
    i = 0
    while i < len(text):
        c = text[i]
        if in_str:
            if c == "\\":
                i += 1
            elif c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == "(":
            depth += 1
            if depth == 2:
                start = i
        elif c == ")":
            if depth == 2:
                out.append((start, i + 1))
            depth -= 1
        i += 1
    return out


def head(block):
    return re.match(r"\((\w+)", block).group(1)


UUID_N = [0]


def new_uuid():
    UUID_N[0] += 1
    return "d7c00000-0000-4000-8000-%012x" % UUID_N[0]


def renew_uuids(block):
    return re.sub(r'\(uuid "[^"]*"\)', lambda m: '(uuid "%s")' % new_uuid(), block)


def set_at(block, x, y, rot=None):
    """The footprint's own (at x y [rot])."""
    def rep(m):
        r = m.group(3) if rot is None else (" %g" % rot if rot else "")
        return "(at %g %g%s)" % (x, y, r or "")
    return re.sub(r"\(at ([-\d.]+) ([-\d.]+)((?: [-\d.]+)?)\)", rep, block, count=1)


def set_reference(block, ref):
    return re.sub(r'\(property "Reference" "[^"]*"', '(property "Reference" "%s"' % ref, block, count=1)


def set_fpid(block, fpid):
    return re.sub(r'^\(footprint "[^"]*"', '(footprint "%s"' % fpid, block, count=1)


def drop_nets(block):
    return re.sub(r'\s*\(net \d+ "[^"]*"\)', "", block)


def sub1(block, old, new):
    assert old in block, old
    return block.replace(old, new, 1)


def pad_block(block, number):
    i = block.index('(pad "%s"' % number)
    depth = 0
    for j in range(i, len(block)):
        if block[j] == "(":
            depth += 1
        elif block[j] == ")":
            depth -= 1
            if depth == 0:
                return i, j + 1
    raise ValueError


def edit_pad(block, number, fn):
    i, j = pad_block(block, number)
    return block[:i] + fn(block[i:j]) + block[j:]


def add_to_pad_angles(block, delta):
    def rep(m):
        a = float(m.group(3) or 0) + delta
        a = a % 360
        return "(at %s %s%s)" % (m.group(1), m.group(2), (" %g" % a) if a else "")
    out = []
    pos = 0
    for m in re.finditer(r'\(pad "[^"]*"', block):
        out.append(block[pos:m.start()])
        i, j = pad_block(block[m.start():], re.match(r'\(pad "([^"]*)"', block[m.start():]).group(1))
        pad = block[m.start():m.start() + j]
        out.append(re.sub(r"\(at ([-\d.]+) ([-\d.]+)((?: [-\d.]+)?)\)", rep, pad, count=1))
        pos = m.start() + j
    out.append(block[pos:])
    return "".join(out)


def flip_side(block):
    """Crude flip: B.Cu side, F./B. layers swapped, local y negated (the reference decides)."""
    block = block.replace('"F.', '"@@.').replace('"B.', '"F.').replace('"@@.', '"B.')
    block = re.sub(r"\((start|end|center|mid|xy) ([-\d.]+) ([-\d.]+)\)",
                   lambda m: "(%s %s %g)" % (m.group(1), m.group(2), -float(m.group(3))), block)
    return block


SHAPES = """(footprint "Shapes"
	(version 20241229)
	(generator "pcbnew")
	(generator_version "9.0")
	(layer "F.Cu")
	(property "Reference" "REF**" (at 0 -4 0) (layer "F.SilkS") (uuid "UUID") (effects (font (size 1 1) (thickness 0.15))))
	(property "Value" "Shapes" (at 0 4 0) (layer "F.Fab") (uuid "UUID") (effects (font (size 1 1) (thickness 0.15))))
	(attr smd)
	(fp_arc (start -2 0) (mid 0 -2) (end 2 0) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "UUID"))
	(fp_poly (pts (xy -1 -1) (xy 1 -1) (xy 1.5 1) (xy -1 1)) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.SilkS") (uuid "UUID"))
	(fp_rect (start -3 -3) (end 3 3) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd") (uuid "UUID"))
	(fp_circle (center 0 0) (end 0.5 0) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab") (uuid "UUID"))
	(fp_curve (pts (xy 0 0) (xy 1 1) (xy 2 1) (xy 3 0)) (stroke (width 0.1) (type solid)) (layer "F.Fab") (uuid "UUID"))
	(fp_line (start -2 2.5) (end 2 2.5) (stroke (width 0.2) (type solid)) (layer "F.Cu") (uuid "UUID"))
	(pad "1" smd roundrect (at -1.5 0) (size 1 1.5) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25) (uuid "UUID"))
	(pad "2" smd custom (at 1.5 0) (size 0.5 0.5) (layers "F.Cu" "F.Mask")
		(options (clearance outline) (anchor circle))
		(primitives (gr_poly (pts (xy 0 -1) (xy 1 0) (xy 0 1)) (width 0.1) (fill yes)))
		(uuid "UUID"))
	(pad "3" smd roundrect (at 0 1.8) (size 1 0.8) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.2)
		(chamfer_ratio 0.25) (chamfer top_left bottom_right) (uuid "UUID"))
	(embedded_fonts no)
)
"""

ZONEFP = """(footprint "ZoneFp"
	(version 20241229)
	(generator "pcbnew")
	(generator_version "9.0")
	(layer "F.Cu")
	(property "Reference" "REF**" (at 0 -4 0) (layer "F.SilkS") (uuid "UUID") (effects (font (size 1 1) (thickness 0.15))))
	(property "Value" "ZoneFp" (at 0 4 0) (layer "F.Fab") (uuid "UUID") (effects (font (size 1 1) (thickness 0.15))))
	(attr smd)
	(pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "UUID"))
	(zone (net 0) (net_name "") (layer "F.Cu") (uuid "UUID") (name "keepout") (hatch edge 0.5)
		(priority 1)
		(connect_pads (clearance 0))
		(min_thickness 0.25)
		(filled_areas_thickness no)
		(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))
		(placement (enabled no) (sheetname ""))
		(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
		(polygon (pts (xy -2 -2) (xy 2 -2) (xy 2 2) (xy -2 2))))
	(embedded_fonts no)
)
"""


def lib_text(template):
    return re.sub(r'\(uuid "UUID"\)', lambda m: '(uuid "%s")' % new_uuid(), template)


def board_instance(lib, fpid, ref, x, y, rot=0):
    """A board footprint from library text placed at (x, y) (rotation: pads only)."""
    body = lib
    body = re.sub(r'\s*\((version|generator|generator_version) [^)]*\)', "", body)
    body = set_fpid(body, fpid)
    body = body.replace('(layer "F.Cu")', '(layer "F.Cu")\n\t(uuid "%s")\n\t(at %g %g%s)'
                        % (new_uuid(), x, y, (" %g" % rot) if rot else ""), 1)
    body = set_reference(body, ref)
    if rot:
        body = add_to_pad_angles(body, rot)
    return renew_uuids(body)


def cases(board_fps):
    """(reference, block) of every crafted footprint."""
    r1 = drop_nets(next(b for b in board_fps if '"Reference" "R1"' in b))
    out = []
    grid = iter((40 + 12 * (i % 10), 40 + 10 * (i // 10)) for i in range(200))

    def r_case(ref, fn=lambda b: b, fpid=None):
        x, y = next(grid)
        b = renew_uuids(set_reference(set_at(r1, x, y), ref))
        if fpid is not None:
            b = set_fpid(b, fpid)
        out.append(fn(b))

    R = "R_Axial_DIN0207_L6.3mm_D2.5mm_P7.62mm_Horizontal"
    r_case("M01")                                                     # identical
    r_case("M02", lambda b: edit_pad(b, "1", lambda p: sub1(p, "(at 0 0 270)", "(at 0.1 0 270)")))
    r_case("M03", lambda b: edit_pad(b, "1", lambda p: sub1(p, "(at 0 0 270)", "(at 0.000005 0 270)")))
    r_case("M04", lambda b: edit_pad(b, "1", lambda p: sub1(p, "(size 1.6 1.6)", "(size 1.7 1.6)")))
    r_case("M05", lambda b: edit_pad(b, "1", lambda p: sub1(p, "(drill 0.8)", "(drill 0.9)")))
    r_case("M06", lambda b: edit_pad(b, "1", lambda p: sub1(p, "thru_hole circle", "thru_hole rect")))
    r_case("M07", lambda b: edit_pad(b, "1", lambda p: sub1(p, '(layers "*.Cu" "*.Mask")', '(layers "*.Cu")')))
    r_case("M08", lambda b: edit_pad(b, "1", lambda p: sub1(p, '(pad "1"', '(pad "3"')))
    r_case("M09", lambda b: edit_pad(b, "1", lambda p: sub1(p, "thru_hole circle", "np_thru_hole circle")))
    r_case("M10", lambda b: edit_pad(b, "1", lambda p: sub1(p, "(at 0 0 270)", "(at 0 0 90)")))
    r_case("M11", lambda b: edit_pad(b, "2", lambda p: sub1(p, "(at 7.62 0 270)", "(at 7.62 0)")))
    r_case("M12", lambda b: edit_pad(b, "1", lambda p: sub1(p, "(drill 0.8)", "(drill 0.8)\n\t\t\t(zone_connect 0)")))
    r_case("M13", lambda b: edit_pad(b, "1", lambda p: sub1(p, "(drill 0.8)", "(drill 0.8)\n\t\t\t(clearance 0.5)")))
    r_case("M14", lambda b: sub1(b, "(start 0.54 1.37)", "(start 0.59 1.37)"))
    r_case("M15", lambda b: sub1(b, "(width 0.12)", "(width 0.2)"))
    r_case("M16", lambda b: re.sub(r"\(fp_line\s*\(start 0.54 1.04\)\s*\(end 0.54 1.37\).*?\(uuid \"[^\"]*\"\)\s*\)", "", b, count=1, flags=re.S))
    r_case("M17", lambda b: sub1(b, "(at 0.381 -2.37 90)", "(at 1.381 -3.37 90)"))
    r_case("M18", lambda b: sub1(b, "R_Axial_DIN0207_L6.3mm_D2.5mm_P7.62mm_Horizontal.wrl", "R_Other.wrl"))
    r_case("M19", lambda b: sub1(b, "(attr through_hole)", "(attr smd)"))
    r_case("M20", lambda b: sub1(b, "(attr through_hole)", "(attr through_hole exclude_from_bom)"))
    r_case("M21", lambda b: sub1(b, "(attr through_hole)", '(attr through_hole)\n\t\t(net_tie_pad_groups "1, 2")'))
    r_case("M22", lambda b: add_to_pad_angles(set_at(b, *next(grid), rot=0), 90))
    r_case("M23", lambda b: add_to_pad_angles(set_at(b, *next(grid), rot=-45), 45))
    r_case("M24", lambda b: set_at(flip_side(b), *next(grid), rot=90))
    r_case("M25", fpid="NotInTable:" + R)
    r_case("M26", fpid="Disabled:" + R)
    r_case("M27", fpid="Missing:" + R)
    r_case("M28", fpid="Bad:" + R)
    r_case("M29", fpid="NestedLib:" + R)
    r_case("M30", fpid="Footprints:NoSuchFootprint")
    # M31: a row of another plugin type (Eagle) is not ported; the reference loads an Eagle
    # row whose URI is a directory as an empty library ("not found in library").
    r_case("M32", fpid="NoType:" + R)
    r_case("M33", fpid="EnvLib:" + R)
    r_case("M34", fpid="RelLib:" + R)
    r_case("M35", fpid=R)
    r_case("M36", fpid="GlobalLib:" + R)
    r_case("M37", fpid="GlobalDisabled:" + R)
    r_case("M38", fpid="Nested Missing:" + R)
    r_case("M39", fpid="Esc{slash}Lib:" + R)

    shapes = lib_text(SHAPES)
    zonefp = lib_text(ZONEFP)

    def s_case(ref, fn=lambda b: b, rot=0, lib=shapes, name="Shapes"):
        x, y = next(grid)
        out.append(fn(board_instance(lib, "Footprints:" + name, ref, x, y, rot)))

    s_case("S01")
    s_case("S02", rot=30)
    s_case("S03", rot=90)
    s_case("S04", lambda b: sub1(b, "(mid 0 -2)", "(mid 0 -2.1)"))
    s_case("S05", lambda b: sub1(b, "(xy 1.5 1)", "(xy 1.6 1)"))
    s_case("S06", lambda b: sub1(b, "(start -3 -3) (end 3 3)", "(start 3 3) (end -3 -3)"))   # same rect
    s_case("S07", lambda b: sub1(b, "(start -3 -3) (end 3 3)", "(start -3 -3) (end 3.1 3)"))
    s_case("S08", lambda b: sub1(b, "(center 0 0) (end 0.5 0)", "(center 0 0) (end 0.6 0)"))
    s_case("S09", lambda b: sub1(b, "(xy 1 1) (xy 2 1)", "(xy 1 1.2) (xy 2 1)"))
    s_case("S10", lambda b: sub1(b, "(width 0.2)", "(width 0.25)"))                           # copper stroke
    s_case("S11", lambda b: sub1(b, "(start -2 2.5) (end 2 2.5)", "(start 2 2.5) (end -2 2.5)"))  # reversed segment
    s_case("S12", lambda b: sub1(b, "(roundrect_rratio 0.25)", "(roundrect_rratio 0.3)"))
    s_case("S13", lambda b: sub1(b, "(xy 1 0) (xy 0 1)", "(xy 1.2 0) (xy 0 1)"))               # custom primitive
    s_case("S14", lambda b: sub1(b, "(chamfer_ratio 0.25)", "(chamfer_ratio 0.3)"))
    s_case("S15", lambda b: sub1(b, "(chamfer top_left bottom_right)", "(chamfer top_left)"))
    s_case("S16", lambda b: sub1(b, "(fill yes) (layer \"F.SilkS\")", "(fill no) (layer \"F.SilkS\")"))
    s_case("S17", rot=45)

    def z_case(ref, fn=lambda b: b):
        b = board_instance(zonefp, "Footprints:ZoneFp", ref, 0, 0)
        out.append(fn(b))

    z_case("Z01")
    z_case("Z02", lambda b: sub1(b, "(priority 1)", "(priority 2)"))
    z_case("Z03", lambda b: sub1(b, "(xy 2 2)", "(xy 2.1 2)"))
    z_case("Z04", lambda b: sub1(b, "(tracks not_allowed)", "(tracks allowed)"))
    z_case("Z05", lambda b: sub1(b, '(name "keepout")', '(name "other")'))
    z_case("Z06", lambda b: re.sub(r"\s*\(zone .*?\(polygon \(pts [^)]*\)[^)]*\)\)\)", "", b, count=1, flags=re.S))
    return out


GLOBAL_TABLE = """(fp_lib_table
  (version 7)
  (lib (name "Footprints")(type "KiCad")(uri "${KIPRJMOD}/does-not-exist.pretty")(options "")(descr "shadowed by the project row"))
  (lib (name "GlobalLib")(type "KiCad")(uri "${KIPRJMOD}/footprints.pretty")(options "")(descr ""))
  (lib (name "GlobalDisabled")(type "KiCad")(uri "${KIPRJMOD}/footprints.pretty")(options "")(descr "")(disabled))
)
"""

# legacy form: no version, unquoted tokens, a member list with spaces
PROJECT_TABLE = """(fp_lib_table
  (lib (name Footprints)(type KiCad)(uri ${KIPRJMOD}/footprints.pretty)(options "")(descr ""))
  (lib (name "Disabled")(type "KiCad")(uri "${KIPRJMOD}/footprints.pretty")(options "")(descr "")(disabled))
  (lib (name "Missing")(type "KiCad")(uri "${KIPRJMOD}/missing.pretty")(options "")(descr ""))
  (lib (name "Bad")(type "KiCad")(uri "${KIPRJMOD}/bad.pretty")(options "")(descr ""))
  (lib (name "NoType")(type "Nonsense")(uri "${KIPRJMOD}/footprints.pretty")(options "")(descr ""))
  (lib (name "EnvLib")(type "kicad")(uri "$(KIPRJMOD)/footprints.pretty")(options "")(descr ""))
  (lib (name "RelLib")(type "KiCad")(uri "footprints.pretty")(options "")(descr ""))
  (lib (name "Esc/Lib")(type "KiCad")(uri "${KIPRJMOD}/footprints.pretty")(options "")(descr ""))
  (lib (name "Sub")(type "Table")(uri "sub/fp-lib-table")(options "")(descr ""))
  (lib (name "SubMissing")(type "Table")(uri "sub/none/fp-lib-table")(options "")(descr ""))
)
"""

SUB_TABLE = """(fp_lib_table
  (version 7)
  (lib (name "NestedLib")(type "KiCad")(uri "${KIPRJMOD}/footprints.pretty")(options "")(descr ""))
  (lib (name "Nested Missing")(type "KiCad")(uri "${KIPRJMOD}/sub/missing.pretty")(options "")(descr ""))
)
"""


def write_data():
    if os.path.isdir(DATA):
        shutil.rmtree(DATA)
    os.makedirs(os.path.join(DATA, "footprints.pretty"))
    os.makedirs(os.path.join(DATA, "bad.pretty"))
    os.makedirs(os.path.join(DATA, "sub"))
    os.makedirs(os.path.join(DATA, "global"))

    src = open(os.path.join(DEMO, "ecc83-pp.kicad_pcb")).read()
    spans = blocks(src)
    keep = []
    fps = []
    for s, e in spans:
        b = src[s:e]
        h = head(b)
        if h in ("segment", "via", "arc", "zone", "group", "gr_text", "dimension"):
            continue
        if h == "footprint":
            fps.append(b)
        keep.append(b)
    root_head = src[:spans[0][0]]
    crafted = cases(fps)
    board = root_head + "\n\t".join(keep + crafted) + "\n)\n"
    # compact the indentation (the parser does not care)
    board = re.sub(r"\n\t+", "\n", board)
    open(os.path.join(DATA, BOARD), "w").write(board)

    lib_src = os.path.join(DEMO, "footprints.pretty")
    for name in sorted(os.listdir(lib_src)):
        text = open(os.path.join(lib_src, name)).read()
        if name.startswith("C_Disc"):
            # library side difference: the board's C_Disc no longer matches
            text = text.replace("(at 5 0)", "(at 5.1 0)", 1)
        open(os.path.join(DATA, "footprints.pretty", name), "w").write(re.sub(r"\n\t+", "\n", text))
    open(os.path.join(DATA, "footprints.pretty", "Shapes.kicad_mod"), "w").write(lib_text(SHAPES))
    open(os.path.join(DATA, "footprints.pretty", "ZoneFp.kicad_mod"), "w").write(lib_text(ZONEFP))
    open(os.path.join(DATA, "footprints.pretty", "notes.txt"), "w").write("not a footprint\n")
    shutil.copy(os.path.join(DATA, "footprints.pretty", "R_Axial_DIN0207_L6.3mm_D2.5mm_P7.62mm_Horizontal.kicad_mod"),
                os.path.join(DATA, "bad.pretty"))
    open(os.path.join(DATA, "bad.pretty", "Broken.kicad_mod"), "w").write("(footprint \"Broken\"\n")
    open(os.path.join(DATA, "fp-lib-table"), "w").write(PROJECT_TABLE)
    open(os.path.join(DATA, "sub", "fp-lib-table"), "w").write(SUB_TABLE)
    open(os.path.join(DATA, "global", "fp-lib-table"), "w").write(GLOBAL_TABLE)
    print("wrote", DATA)


def write_golden():
    tmp = tempfile.mkdtemp(prefix="drc_lp_")
    try:
        work = os.path.join(tmp, "drc_lp")
        shutil.copytree(DATA, work, ignore=shutil.ignore_patterns("global"))
        home = os.path.join(tmp, "home")
        cfg = os.path.join(home, ".config", "kicad", "10.99")
        os.makedirs(cfg)
        shutil.copy(os.path.join(DATA, "global", "fp-lib-table"), cfg)
        env = dict(os.environ, KICAD_ORACLE_HOME=home)
        out = os.path.join(tmp, "out.json")
        subprocess.run([REF_CLI, "pcb", "drc", "--format", "json", "--severity-all", "--units", "mm",
                        "-o", out, BOARD], cwd=work, env=env, check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        doc = json.load(open(out))
        lib = [v for v in doc["violations"] if v["type"].startswith("lib_footprint")]
        with open(GOLDEN, "w") as f:
            json.dump(lib, f, indent=1, sort_keys=True)
            f.write("\n")
        print("wrote", GOLDEN, len(lib), "entries")
    finally:
        shutil.rmtree(tmp)


if __name__ == "__main__":
    {"data": write_data, "golden": write_golden}[sys.argv[1]]()
