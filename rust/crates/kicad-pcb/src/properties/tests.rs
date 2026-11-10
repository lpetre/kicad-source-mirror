// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Tests of the property machinery. The cases of `qa/tests/common/test_property.cpp` that
//! exercise `PROPERTY_MANAGER` logic are ported on a private manager whose classes stand in for
//! the test's `A` .. `E` (`SetGet`, `VirtualMethods` and `IncorrectType` test setters and the
//! `wxAny` type checks, which are not ported: setters are tier 3 and the Rust getters are
//! typed). The board-class tables themselves are checked against the C++ by
//! `tests/props_oracle.rs`.

use super::*;

// Stand-ins: A = EdaShape, B = EdaText (derives A), C = EdaGroup, D = BoardItem (derives A, C),
// E = BoardConnectedItem (registered without `InheritsAfter`, as in the C++ test).
const A: ClassId = ClassId::EdaShape;
const B: ClassId = ClassId::EdaText;
const C: ClassId = ClassId::EdaGroup;
const D: ClassId = ClassId::BoardItem;
const E: ClassId = ClassId::BoardConnectedItem;

fn int_getter(_: ItemRef<'_>) -> PropertyValue {
    PropertyValue::Int(0)
}

fn bool_getter(_: ItemRef<'_>) -> PropertyValue {
    PropertyValue::Bool(false)
}

/// The descriptors of the C++ test (`CLASS_A_DESC` .. `CLASS_E_DESC`), with the
/// `PCB_LAYER_ID` map standing in for `enum_glob` / `D::enum_class`.
fn fixture() -> PropertyManager {
    let mut pm = PropertyManager::default();

    pm.add_property(Property::new("A", A, ValueType::Int, true, int_getter), "");
    pm.add_property(Property::new("A2", A, ValueType::Int, true, int_getter), "");
    pm.add_property(
        Property::new("point", A, ValueType::Int, true, int_getter),
        "",
    );

    pm.inherits_after(B, A);
    pm.add_property(Property::new("C", B, ValueType::Int, true, int_getter), "");

    pm.add_property(
        Property::new("bool", C, ValueType::Bool, true, bool_getter),
        "",
    );
    pm.add_property(
        Property::new("new", C, ValueType::Int, true, int_getter),
        "",
    );

    pm.add_property(
        Property::new_enum(
            "enumGlob",
            D,
            ValueType::Enum(EnumId::ShapeT),
            true,
            int_getter,
        ),
        "",
    );
    pm.add_property(
        Property::new("point_alias", D, ValueType::Int, true, int_getter).with_base(A),
        "",
    );
    pm.replace_property(
        C,
        "bool",
        Property::new("replaced_bool", D, ValueType::Bool, true, bool_getter).with_base(C),
        "",
    );
    // lines below are needed to indicate multiple inheritance
    pm.add_type_cast(D, A);
    pm.add_type_cast(D, C);
    pm.inherits_after(D, A);
    pm.inherits_after(D, C);
    pm.add_property(
        Property::new("cond", D, ValueType::Int, true, int_getter),
        "",
    );

    let prop = Property::new_enum(
        "enumGlob",
        E,
        ValueType::Enum(EnumId::ShapeT),
        true,
        int_getter,
    )
    .with_base(D);
    pm.replace_property(D, "enumGlob", prop, "")
        .set_choices(vec![(0, "T1".to_string()), (4, "T3".to_string())]);

    pm.rebuild();
    pm
}

/// `NotexistingProperties`.
#[test]
fn notexisting_properties() {
    let pm = fixture();
    assert!(pm.get_property(D, "does not exist").is_none());
    assert!(pm.get_property(D, "neither").is_none());
}

/// `TypeCasting`: D casts to both of its bases.
#[test]
fn type_casting() {
    let pm = fixture();
    assert!(pm.has_type_cast(D, A));
    assert!(pm.has_type_cast(D, C));
    assert!(pm.is_of_type(D, A) && pm.is_of_type(D, C) && !pm.is_of_type(E, A));
    assert!(!pm.is_of_type(A, D));
}

/// `EnumGlob`: the choices come from the `ENUM_MAP`, an unmapped value does not convert to a
/// string, a mapped one does.
#[test]
fn enum_glob() {
    let pm = fixture();
    let prop = pm.property(pm.get_property(D, "enumGlob").expect("enumGlob"));
    assert!(prop.has_choices());
    assert_eq!(prop.choices(), EnumId::ShapeT.map().choices_owned());

    assert_eq!(
        PropertyValue::Enum(EnumId::ShapeT, -1).enum_as_string(),
        None
    );
    assert_eq!(
        PropertyValue::Enum(EnumId::ShapeT, 0).enum_as_string(),
        Some("Segment")
    );
    // A value equal to the undefined value converts even when unmapped.
    assert_eq!(
        PropertyValue::Enum(EnumId::PadDrillPostMachiningMode, 0).enum_as_string(),
        Some("UNDEFINED")
    );
}

/// `Rename`: a replaced property is gone under its old name.
#[test]
fn rename() {
    let pm = fixture();
    assert!(pm.get_property(D, "bool").is_none());
    assert!(pm.get_property(D, "replaced_bool").is_some());
    // The base class keeps it.
    assert!(pm.get_property(C, "bool").is_some());
}

/// `Alias`: a derived class may register a property under another name with a base getter.
#[test]
fn alias() {
    let pm = fixture();
    let p = pm.property(pm.get_property(D, "point_alias").expect("alias"));
    assert_eq!(p.owner, D);
    assert_eq!(p.base, A);
    assert!(pm.get_property(D, "point").is_some());
}

/// `AlternativeEnum`: a replacing `PROPERTY_ENUM` with its own subset of choices.
#[test]
fn alternative_enum() {
    let pm = fixture();
    let prop = pm.property(pm.get_property(E, "enumGlob").expect("enumGlob"));
    assert!(prop.has_choices());
    assert_eq!(
        prop.choices(),
        vec![(0, "T1".to_string()), (4, "T3".to_string())]
    );
}

/// `DuplicateProperty`: a second registration under the same name returns the existing one.
#[test]
fn duplicate_property() {
    let mut pm = fixture();
    let original = pm.get_property(B, "C").expect("C");
    let count = pm.props.len();
    let returned = pm.add_property(Property::new("C", B, ValueType::Int, false, int_getter), "");
    assert!(returned.has_setter, "the existing property is returned");
    assert_eq!(pm.props.len(), count);
    pm.rebuild();
    assert_eq!(pm.get_property(B, "C"), Some(original));
}

/// The display order (`collectPropsRecur`: own properties, then the bases in reverse
/// `InheritsAfter` order, each block before the previous one) and the case insensitive lookup
/// the rule language relies on.
#[test]
fn display_order_and_lookup() {
    let pm = fixture();
    let names: Vec<&str> = pm
        .properties(D)
        .iter()
        .map(|p| pm.property(*p).name)
        .collect();
    assert_eq!(
        names,
        [
            "A",
            "A2",
            "point",
            "new",
            "enumGlob",
            "point_alias",
            "replaced_bool",
            "cond"
        ]
    );
    let order = pm.display_order(D);
    let values: Vec<i32> = pm.properties(D).iter().map(|p| order[p]).collect();
    assert_eq!(values, [-5, -4, -3, -2, 0, 1, 2, 3]);
    assert!(pm.get_property(D, "ENUMGLOB").is_some());
}

/// The board tables: names are unique case-insensitively within a class except `Text` of the
/// dimension classes (`EDA_TEXT::Text` and `PCB_DIMENSION_BASE::Text`), where the oracle table
/// shows `GetProperty` returning the dimension's (`lookup=0` for the `EDA_TEXT` one).
#[test]
fn board_tables_lookup_is_unique() {
    let pm = property_manager();

    for cls in pm.all_classes() {
        for pid in pm.properties(cls.type_) {
            let p = pm.property(pid);
            let found = pm.get_property(cls.type_, p.name);
            assert_eq!(found, pm.get_property(cls.type_, &p.name.to_uppercase()));

            if p.name == "Text"
                && p.owner == ClassId::EdaText
                && pm.is_of_type(cls.type_, ClassId::PcbDimensionBase)
            {
                assert_eq!(
                    pm.property(found.expect("Text")).owner,
                    ClassId::PcbDimensionBase
                );
            } else {
                assert_eq!(found, Some(pid), "{}", p.name);
            }
        }
    }

    // `A.Net_Class`: the rule language replaces '_' by ' '.
    assert!(pm.get_property(ClassId::Pad, "net class").is_some());
    assert!(pm.get_property(ClassId::PcbVia, "Via Type").is_some());
    assert!(
        pm.get_property(ClassId::Pad, "Layer").is_none(),
        "masked for PAD"
    );
    assert!(pm.is_of_type(ClassId::PcbDimOrthogonal, ClassId::PcbDimAligned));
    assert!(
        pm.derived_classes(ClassId::BoardConnectedItem)
            .contains(&ClassId::PcbTablecell)
    );
}
