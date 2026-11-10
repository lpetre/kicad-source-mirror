# PROP-1: Board item property tables (PROPERTY_MANAGER port for pcbnew items)

Crate: kicad-pcb

KiCad registers named, typed properties for every board item class (static `*_DESC` structs
using PROPERTY_MANAGER / PROPERTY / PROPERTY_ENUM in pcbnew/pad.cpp, pcb_track.cpp, zone.cpp,
footprint.cpp, pcb_shape.cpp, pcb_text.cpp, pcb_textbox.cpp, pcb_field.cpp, pcb_dimension.cpp,
pcb_group.cpp, pcb_generator.cpp, pcb_table*.cpp, pcb_barcode.cpp, pcb_reference_image.cpp,
board_item.cpp, board_connected_item.cpp, common/eda_shape.cpp, common/eda_text.cpp, ...; the
machinery is include/properties/property.h, property_mgr.h and common/properties/*.cpp). The
custom DRC rule language reads them by name (`A.Net_Class`, `A.Thickness`, `A.Pad_Type ==
'SMD'`, `L == 'F.Cu'`), so they are part of the rule-file contract.

You own a new module tree rust/crates/kicad-pcb/src/properties/ (mod.rs + one file per item
family) and its `pub mod properties;` line in lib.rs (sanctioned edit of that frozen file).

Port:
1. The PROPERTY_MANAGER model as static data + lookup: for every class (TYPE_HASH), the list
   of properties in registration order with: name (the English registration string, e.g.
   "Net Class", "Position X"), display type (PROPERTY_DISPLAY: distance, area, coordinate,
   degrees, time ...), value type (int, double, bool, string, enum with its ENUM_MAP entries -
   both the stored value and the display string - , EDA_ANGLE, layer id, LSET, COLOR4D,
   std::optional<..>), the availability / writability predicates, the group, base-class
   inheritance (InheritsAfter, AddTypeCast), property overrides and hides (OverrideAvailability,
   Mask, ReplaceProperty), and the lookups GetProperty(type, name) incl. the "name with
   underscores instead of spaces" matching the expression binding uses, GetProperties(type) in
   the C++ order, IsOfType / GetAllClasses / derived-class enumeration.
2. For every registered property a getter on the Rust model: `fn(ItemRef) -> PropertyValue`
   calling the same model method the C++ getter calls (many are frame-dependent: take them
   through ItemRef / ctx exactly as the C++ member getter would compute them). Setters are
   tier 3: record them as data (has_setter) but do not port them.
3. ENUM_MAP contents for every enum used by properties (pad shape / type / property, via
   type, zone connection, fill types, layer names incl. the wxPG choices as the C++ builds
   them for PCB_LAYER_ID, dimension enums, ...): value <-> string exactly as registered
   (some are translated strings: use the English source text).
Model functions that are still port_pending! stubs are called anyway.

Verification: extend the reference dump tool (rust/tools/pcb-dump: one new source file with a
`props` section, registered in main.cpp and build.sh; minimal edits; build under the
machine-wide flock as PORTING.md section 7 requires; document it in
rust/docs/PCB_DUMP_FORMAT.md) that prints (a) the complete static table: every class with its
properties in order (name, type name, display, enum choices, availability flags) - `pcb-dump
props-table` - and (b) per board item, every available property as `props|KEY|Name=value`
with values rendered the way PCBEXPR turns a property into a value (see
pcbnew/pcbexpr_evaluator.cpp PCBEXPR_VAR_REF::GetValue: enums as strings, ints, doubles,
strings, layer names). Mirror both in Rust (src/dump/props.rs + a `props` section in the
dump driver: src/dump has no owner any more, add your file and the minimal hook) and compare
with rust/tools/pcb-dump/compare.py: the static table must be identical; the per-item values
must have zero mismatches on COPIES of at least 30 small corpus boards the Rust parser can
load (footprints and pads are landing right now: tracks, vias, shapes, text, zones and
dimensions load today; pending model stubs show as pending). Port the property-related C++
tests (qa/tests/common/test_property.cpp, qa/tests/pcbnew tests that enumerate properties,
e.g. test_custom_property_roundtrip as far as it is model logic).
