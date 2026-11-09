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

"""Check the delegation shims of kicad-pcb against the C++ headers.

Rule (rust/docs/design/board-model.md 6.2): an `impl Trait for T` block has one shim for
every virtual that the C++ class of T, or one of its C++ bases below the class the trait
ports, overrides - and no other method besides the identity accessors. A missing shim is
not a compile error (the trait default, i.e. the base-class body, is used silently), so
this script compares the two sides:

    python3 crates/kicad-pcb/tools/check_item_shims.py            # every type
    python3 crates/kicad-pcb/tools/check_item_shims.py PcbVia Pad # some types

A Rust type that stands for a whole C++ class family (`PcbGenerator`: `PCB_GENERATOR` and its
subclasses in `pcbnew/generators/*.h`) needs a shim for every virtual that any class of the
family overrides: the shim's inherent method dispatches on the subclass (`FAMILIES`).

Run from `<kicad>/rust`. Exit status 1 if a shim is missing or unexpected. It reads the C++
tree and the Rust sources only; it changes nothing.
"""

import glob
import os
import re
import sys

RUST = os.path.join("crates", "kicad-pcb", "src")
CXX = ".."

# Rust type -> (file of the impl blocks, C++ header, C++ class, C++ bases that are item classes)
TYPES = {
    "PcbTrack": ("pcb_track/mod.rs", "pcbnew/pcb_track.h", "PCB_TRACK", []),
    "PcbArc": ("pcb_track/mod.rs", "pcbnew/pcb_track.h", "PCB_ARC", ["PcbTrack"]),
    "PcbVia": ("pcb_track/mod.rs", "pcbnew/pcb_track.h", "PCB_VIA", ["PcbTrack"]),
    "Pad": ("pad/mod.rs", "pcbnew/pad.h", "PAD", []),
    "Footprint": ("footprint/mod.rs", "pcbnew/footprint.h", "FOOTPRINT", []),
    "Zone": ("zone/mod.rs", "pcbnew/zone.h", "ZONE", []),
    "PcbShape": ("pcb_shape/mod.rs", "pcbnew/pcb_shape.h", "PCB_SHAPE", []),
    "PcbText": ("pcb_text.rs", "pcbnew/pcb_text.h", "PCB_TEXT", []),
    "PcbField": ("pcb_field.rs", "pcbnew/pcb_field.h", "PCB_FIELD", ["PcbText"]),
    "PcbTextbox": ("pcb_textbox.rs", "pcbnew/pcb_textbox.h", "PCB_TEXTBOX", ["PcbShape"]),
    "PcbTablecell": (
        "pcb_tablecell.rs",
        "pcbnew/pcb_tablecell.h",
        "PCB_TABLECELL",
        ["PcbTextbox", "PcbShape"],
    ),
    "PcbTable": ("pcb_table.rs", "pcbnew/pcb_table.h", "PCB_TABLE", []),
    "PcbDimension": (
        "pcb_dimension.rs",
        "pcbnew/pcb_dimension.h",
        "PCB_DIMENSION_BASE",
        ["PcbText"],
    ),
    "PcbBarcode": ("pcb_barcode.rs", "pcbnew/pcb_barcode.h", "PCB_BARCODE", []),
    "PcbReferenceImage": (
        "pcb_reference_image.rs",
        "pcbnew/pcb_reference_image.h",
        "PCB_REFERENCE_IMAGE",
        [],
    ),
    "PcbTarget": ("pcb_target.rs", "pcbnew/pcb_target.h", "PCB_TARGET", []),
    "PcbPoint": ("pcb_point.rs", "pcbnew/pcb_point.h", "PCB_POINT", []),
    "PcbGroup": ("pcb_group.rs", "pcbnew/pcb_group.h", "PCB_GROUP", []),
    "PcbGenerator": ("pcb_generator.rs", "pcbnew/pcb_generator.h", "PCB_GENERATOR", ["PcbGroup"]),
    "PcbMarker": ("pcb_marker.rs", "pcbnew/pcb_marker.h", "PCB_MARKER", []),
    "PcbConstraint": (
        "pcb_constraint.rs",
        "pcbnew/constraints/pcb_constraint.h",
        "PCB_CONSTRAINT",
        [],
    ),
    "Pcb3dBody": ("pcb_3d_body.rs", "pcbnew/pcb_3d_body.h", "PCB_3D_BODY", []),
    "PcbGridItem": ("pcb_grid_item.rs", "pcbnew/pcb_grid_item.h", "PCB_GRID_ITEM", []),
    "PcbDrillMap": ("pcb_drill_map.rs", "pcbnew/pcb_drill_map.h", "PCB_DRILL_MAP", []),
    "NetinfoItem": ("netinfo.rs", "pcbnew/netinfo.h", "NETINFO_ITEM", []),
}

# Rust type -> header globs (relative to the C++ tree) searched for C++ subclasses of the type's
# class, direct or indirect. The Rust struct represents every class found (the subclass is a
# data member, e.g. `PcbGenerator::generator_type`), so an override in any of them needs a
# shim, exactly like an override in the class itself.
FAMILIES = {
    "PcbGenerator": ["pcbnew/pcb_generator.h", "pcbnew/generators/*.h"],
    "PcbDimension": ["pcbnew/pcb_dimension.h"],
    "PcbTable": [
        "pcbnew/pcb_generated_table.h",
        "pcbnew/pcb_drill_chart.h",
        "pcbnew/pcb_stackup_table.h",
    ],
}

# trait -> { C++ virtual : trait method }. A value may be a dict keyed by a substring of the
# C++ parameter list, to tell overloads apart ("" = any other parameter list; None = not in
# the trait).
TRAITS = {
    "BoardItem": {
        "GetParentGroup": "parent_group",
        "GetLayer": "layer",
        "GetLayerSet": "layer_set",
        "IsOnLayer": "is_on_layer",
        "IsOnCopperLayer": "is_on_copper_layer",
        "IsLayerAgnostic": "is_layer_agnostic",
        "FitsEnabledLayers": "fits_enabled_layers",
        "GetPosition": "position",
        "GetCenter": "center",
        "GetBoundingBox": "bounding_box",
        "GetEffectiveShape": "effective_shape",
        "GetEffectiveHoleShape": "effective_hole_shape",
        "TransformShapeToPolygon": "transform_shape_to_polygon",
        "TransformShapeToPolySet": "transform_shape_to_poly_set",
        "HasHole": "has_hole",
        "HasDrilledHole": "has_drilled_hole",
        "IsTented": "is_tented",
        "HasLineStroke": "has_line_stroke",
        "GetStroke": "stroke",
        "HitTest": {"VECTOR2I": "hit_test", "BOX2I": "hit_test_rect", "": None},
        "IsLocked": "is_locked",
        "IsKnockout": "is_knockout",
        "RunOnChildren": "run_on_children",
        "GetItemDescription": "item_description",
        "GetFriendlyName": "friendly_name",
        "LayerMaskDescribe": "layer_mask_describe",
    },
    "BoardConnectedItem": {
        "IsConnected": "is_connected",
        "GetEffectiveNetClass": "effective_netclass",
        "GetOwnClearance": "own_clearance",
        "GetLocalClearance": {"wxString": None, "": "local_clearance"},
        "GetClearanceOverrides": "clearance_overrides",
    },
    "BoardItemMut": {
        "SetPosition": "set_position",
        "Move": "move_by",
        "Rotate": "rotate",
        "Flip": "flip",
        "Mirror": {"SEG": None, "": "mirror"},
        "SetLayer": "set_layer",
        "SetLayerSet": "set_layer_set",
        "Normalize": "normalize",
        "OnFootprintTransformed": "on_footprint_transformed",
        "OnFootprintRescaled": "on_footprint_rescaled",
        "StyleFromSettings": "style_from_settings",
    },
    "EdaTextItem": {
        "GetTextPos": "text_pos",
        "GetTextSize": "text_size",
        "GetTextThickness": "text_thickness",
        "GetTextAngle": "text_angle",
        "GetDrawPos": "draw_pos",
        "GetDrawRotation": "draw_rotation",
        "GetShownText": {"RESOLUTION_CONTEXT aContext,": "shown_text", "": None},
        "getFontMetrics": "font_metrics",
        "SetTextPos": "set_text_pos",
        "SetTextSize": "set_text_size",
        "SetTextThickness": "set_text_thickness",
        "SetTextAngle": "set_text_angle",
        "Offset": "offset",
    },
    "EdaShapeItem": {
        "GetWidth": "width",
        "GetEffectiveWidth": "effective_width",
        "GetHatchLineWidth": "hatch_line_width",
        "GetHatchLineSpacing": "hatch_line_spacing",
        "IsProxyItem": "is_proxy_item",
        "IsFilledForHitTesting": "is_filled_for_hit_testing",
        "GetTopLeft": "top_left",
        "GetBotRight": "bot_right",
        "GetCornersInSequence": "corners_in_sequence",
        "getDrawRotation": "shape_draw_rotation",
        "getMaxError": "shape_max_error",
        "getHatchingKnockouts": "hatching_knockouts",
    },
}

# Methods an impl block may contain without a C++ override: identity accessors.
ACCESSORS = {
    "base",
    "base_mut",
    "type_",
    "class_name",
    "connected",
    "connected_mut",
    "eda_text",
    "eda_text_mut",
    "eda_shape",
    "eda_shape_mut",
}

# Which C++ classes are a BOARD_CONNECTED_ITEM, an EDA_TEXT, an EDA_SHAPE (the trait only
# applies to those; PCB_VIA::GetWidth is not EDA_SHAPE::GetWidth).
CONNECTED = {"PcbTrack", "PcbArc", "PcbVia", "Pad", "Zone", "PcbShape", "PcbTextbox", "PcbTablecell"}
TEXT_HOSTS = {"PcbText", "PcbField", "PcbDimension", "PcbTextbox", "PcbTablecell"}
SHAPE_HOSTS = {"PcbShape", "PcbTextbox", "PcbTablecell"}
# Containers cascade through `Board` methods (board-model.md 8.3) and nets are edited through
# the net list: their C++ overrides of the mutating virtuals are not `BoardItemMut` shims.
NO_ITEM_MUT = {"Footprint", "PcbTable", "PcbGroup", "PcbGenerator", "NetinfoItem"}


def applies(trait, rust_type):
    if trait == "BoardConnectedItem":
        return rust_type in CONNECTED
    if trait == "EdaTextItem":
        return rust_type in TEXT_HOSTS
    if trait == "EdaShapeItem":
        return rust_type in SHAPE_HOSTS
    if trait == "BoardItemMut":
        return rust_type not in NO_ITEM_MUT
    return True


# Overrides that are deliberately not shims, with the reason (reported, not an error).
WAIVED = {}


def class_body(header_text, class_name):
    """The text between the braces of `class <name>`."""
    match = re.search(r"\bclass\s+(?:\w+\s+)?" + class_name + r"\b[^;{]*\{", header_text)
    if not match:
        return None
    depth = 1
    pos = match.end()
    while depth and pos < len(header_text):
        ch = header_text[pos]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        pos += 1
    return header_text[match.end() : pos - 1]


def overrides(body):
    """(name, parameter list) of every member function declared `override` in a class body."""
    text = re.sub(r"//[^\n]*", "", body)
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    text = re.sub(r"\s+", " ", text)
    found = []
    tail = re.compile(r"\s*(?:const\s*)?override\b")
    for match in re.finditer(r"\b(\w+)\s*\(", text):
        # the matching ')' (parameter lists may nest: std::function<void( BOARD_ITEM* )>)
        depth = 1
        pos = match.end()
        while depth and pos < len(text):
            if text[pos] == "(":
                depth += 1
            elif text[pos] == ")":
                depth -= 1
            pos += 1
        if depth == 0 and tail.match(text, pos):
            found.append((match.group(1), text[match.end() : pos - 1]))
    return found


def subclasses(root_class, patterns, cache):
    """[(header, class)] of the classes in the headers matching `patterns` that derive from
    `root_class`, directly or through another class of the same headers; sorted."""
    headers = sorted(
        os.path.relpath(path, CXX)
        for pattern in patterns
        for path in glob.glob(os.path.join(CXX, pattern))
    )
    if not headers:
        raise SystemExit(f"no header matches {' '.join(patterns)}")
    declared = []  # (header, class, bases)
    for header in headers:
        text = read(os.path.join(CXX, header), cache)
        text = re.sub(r"//[^\n]*", "", text)
        for match in re.finditer(r"\bclass\s+(?:\w+\s+)?(\w+)\s*(?:final\s*)?:([^{;]*)\{", text):
            bases = re.findall(r"\b(?:public|protected|private)\s+([\w:]+)", match.group(2))
            declared.append((header, match.group(1), bases))
    family = {root_class}
    found = []
    grown = True
    while grown:
        grown = False
        for header, name, bases in declared:
            if name not in family and family.intersection(bases):
                family.add(name)
                found.append((header, name))
                grown = True
    return sorted(found)


def read(path, cache):
    if path not in cache:
        with open(path, encoding="utf-8") as handle:
            cache[path] = handle.read()
    return cache[path]


def trait_method(mapping, name, params):
    target = mapping.get(name, "absent")
    if target == "absent":
        return None
    if isinstance(target, dict):
        for key, value in target.items():
            if key and key in params:
                return value
        return target.get("")
    return target


def impl_methods(rust_text, trait, rust_type):
    """Method names of `impl <trait> for <type> { .. }`, or None if there is no such block."""
    match = re.search(r"\bimpl\s+" + trait + r"\s+for\s+" + rust_type + r"\s*\{", rust_text)
    if not match:
        return None
    depth = 1
    pos = match.end()
    while depth and pos < len(rust_text):
        ch = rust_text[pos]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        pos += 1
    block = rust_text[match.end() : pos - 1]
    # only functions at the top level of the block
    names = []
    depth = 0
    for line in block.split("\n"):
        if depth == 0:
            fn = re.match(r"\s*fn\s+(\w+)", line)
            if fn:
                names.append(fn.group(1))
        depth += line.count("{") - line.count("}")
    return set(names)


def cxx_overrides_of(rust_type, cache):
    """trait method -> C++ origin, for the class of `rust_type` and its item base classes."""
    _, header, cxx_class, bases = TYPES[rust_type]
    result = {}
    # the class itself, the subclasses folded into the Rust type, then the item base classes
    chain = [(header, cxx_class)]
    if rust_type in FAMILIES:
        chain += subclasses(cxx_class, FAMILIES[rust_type], cache)
    chain += [(TYPES[base][1], TYPES[base][2]) for base in bases]
    for link_header, link_class in chain:
        body = class_body(read(os.path.join(CXX, link_header), cache), link_class)
        if body is None:
            raise SystemExit(f"class {link_class} not found in {link_header}")
        for name, params in overrides(body):
            for trait, mapping in TRAITS.items():
                if not applies(trait, rust_type):
                    continue
                method = trait_method(mapping, name, params)
                if method:
                    result.setdefault((trait, method), f"{link_class}::{name}")
    return result


def check(rust_type, cache):
    rust_file = os.path.join(RUST, TYPES[rust_type][0])
    with open(rust_file, encoding="utf-8") as handle:
        rust_text = handle.read()

    expected = cxx_overrides_of(rust_type, cache)
    problems = []
    notes = []

    for trait in TRAITS:
        methods = impl_methods(rust_text, trait, rust_type)
        wanted = {m: origin for (t, m), origin in expected.items() if t == trait}

        if not applies(trait, rust_type):
            if methods is not None:
                problems.append(f"`impl {trait} for {rust_type}` exists but the C++ class is not one")
            continue

        if methods is None:
            problems.append(f"no `impl {trait} for {rust_type}`")
            continue

        for method, origin in sorted(wanted.items()):
            if method in methods:
                continue
            reason = WAIVED.get((rust_type, method))
            if reason:
                notes.append(f"{trait}::{method} ({origin}) waived: {reason}")
            else:
                problems.append(f"missing shim {trait}::{method} for {origin}")

        for method in sorted(methods - set(wanted) - ACCESSORS):
            problems.append(f"shim {trait}::{method} has no C++ override in {TYPES[rust_type][2]} or its bases")

    return problems, notes


def trait_methods(traits_text, trait):
    """Method names declared in `pub trait <trait>` (traits.rs)."""
    match = re.search(r"\bpub trait\s+" + trait + r"\b[^{]*\{", traits_text)
    depth = 1
    pos = match.end()
    while depth and pos < len(traits_text):
        ch = traits_text[pos]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        pos += 1
    names = []
    depth = 0
    for line in traits_text[match.end() : pos - 1].split("\n"):
        if depth == 0:
            fn = re.match(r"\s*fn\s+(\w+)", line)
            if fn:
                names.append(fn.group(1))
        depth += line.count("{") - line.count("}")
    return set(names)


def check_track_dispatch():
    """`Track` (the tagged PCB_TRACK*) must dispatch every method of the three traits."""
    with open(os.path.join(RUST, "traits.rs"), encoding="utf-8") as handle:
        traits_text = handle.read()
    with open(os.path.join(RUST, "pcb_track", "mod.rs"), encoding="utf-8") as handle:
        rust_text = handle.read()
    problems = []
    for trait in ("BoardItem", "BoardConnectedItem", "BoardItemMut"):
        declared = trait_methods(traits_text, trait)
        dispatched = impl_methods(rust_text, trait, "Track") or set()
        for method in sorted(declared - dispatched):
            problems.append(f"`impl {trait} for Track` does not dispatch {method}")
    return problems


def main():
    wanted = sys.argv[1:] or list(TYPES) + ["Track"]
    cache = {}
    failed = False

    if "Track" in wanted:
        wanted = [w for w in wanted if w != "Track"]
        problems = check_track_dispatch()
        print(f"Track (enum dispatch): {'FAIL' if problems else 'ok'}")
        for line in problems:
            print(f"    {line}")
        failed = bool(problems)

    for rust_type in wanted:
        if rust_type not in TYPES:
            raise SystemExit(f"unknown type {rust_type}; known: {' '.join(TYPES)}")
        problems, notes = check(rust_type, cache)
        status = "FAIL" if problems else "ok"
        print(f"{rust_type}: {status}")
        for line in problems:
            print(f"    {line}")
        for line in notes:
            print(f"    note: {line}")
        failed = failed or bool(problems)

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
