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

//! Rust side of `dump_props.cpp` of the reference dump tool: the `props-table` command (every
//! property class, its ancestors, groups and properties, and every `ENUM_MAP`) and the opt-in
//! `props` section (every property of an item's class with its value as
//! `PCBEXPR_VAR_REF::GetValue` computes it). Format: `docs/PCB_DUMP_FORMAT.md`.

use kicad_core::layer_ids::to_layer_id;

use super::format::{Out, emit, v_f64, v_layer, v_str};
use crate::item_ref::ItemRef;
use crate::properties::{
    ClassId, EnumId, Property, PropertyValue, ValueType, class_of, property_manager,
};

/// `PCBEXPR_PROPERTY_KIND` (`pcbnew/pcbexpr_evaluator.h`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PropertyKind {
    IntKind,
    OptionalInt,
    Unsigned,
    LongLong,
    Double,
    OptionalDouble,
    BoolKind,
    String,
    Enum,
    Angle,
    Color,
    Unsupported,
}

impl PropertyKind {
    fn cxx_name(self) -> &'static str {
        match self {
            PropertyKind::IntKind => "INT_KIND",
            PropertyKind::OptionalInt => "OPTIONAL_INT",
            PropertyKind::Unsigned => "UNSIGNED",
            PropertyKind::LongLong => "LONG_LONG",
            PropertyKind::Double => "DOUBLE",
            PropertyKind::OptionalDouble => "OPTIONAL_DOUBLE",
            PropertyKind::BoolKind => "BOOL_KIND",
            PropertyKind::String => "STRING",
            PropertyKind::Enum => "ENUM",
            PropertyKind::Angle => "ANGLE",
            PropertyKind::Color => "COLOR",
            PropertyKind::Unsupported => "UNSUPPORTED",
        }
    }
}

/// `PCBEXPR_VAR_REF::ClassifyProperty`.
pub(crate) fn classify_property(p: &Property) -> PropertyKind {
    match p.value_type {
        ValueType::Int => PropertyKind::IntKind,
        ValueType::OptInt => PropertyKind::OptionalInt,
        ValueType::Unsigned => PropertyKind::Unsigned,
        ValueType::LongLong => PropertyKind::LongLong,
        ValueType::Double => PropertyKind::Double,
        ValueType::OptDouble => PropertyKind::OptionalDouble,
        ValueType::Bool => PropertyKind::BoolKind,
        ValueType::String => PropertyKind::String,
        _ if p.has_choices() => PropertyKind::Enum,
        ValueType::Angle => PropertyKind::Angle,
        ValueType::Color => PropertyKind::Color,
        _ => PropertyKind::Unsupported,
    }
}

fn v_choices(choices: &[(i32, String)]) -> String {
    let items: Vec<String> = choices
        .iter()
        .map(|(v, l)| format!("{v}:{}", v_str(l)))
        .collect();
    format!("[{}]", items.join(","))
}

fn v01(b: bool) -> String {
    if b { "1" } else { "0" }.to_string()
}

/// `DumpPropsTable`: the `props-table` command.
pub fn dump_props_table(hash: bool) -> String {
    let _quiet = super::SilentPanics::new();
    let mut o = Out::new(hash);
    let pm = property_manager();

    let mut classes = pm.all_classes();
    classes.sort_by_key(|c| c.type_.cxx_name());

    for cls in &classes {
        let id = cls.type_;
        o.scope("propclass", id.cxx_name(), |o| {
            emit!(o, "registered", v_str(&cls.name));

            let mut ancestors: Vec<&str> = ClassId::ALL
                .iter()
                .filter(|other| **other != id && pm.is_of_type(id, **other))
                .map(|c| c.cxx_name())
                .collect();
            ancestors.sort();
            emit!(o, "ancestors", format!("[{}]", ancestors.join(",")));

            let groups: Vec<String> = pm
                .group_display_order(id)
                .iter()
                .map(|g| v_str(g))
                .collect();
            emit!(o, "groups", format!("[{}]", groups.join(",")));
            emit!(o, "count", pm.all_properties_count(id).to_string());

            let props = pm.properties(id);
            emit!(o, "distinct", props.len().to_string());
            let order = pm.display_order(id);

            for (idx, pid) in props.iter().enumerate() {
                let p = pm.property(*pid);
                emit!(o, idx.to_string(), {
                    let mut v = String::new();
                    v += &format!("name={}", v_str(p.name));
                    v += &format!(";order={}", order[pid]);
                    v += &format!(";owner={}", p.owner.cxx_name());
                    v += &format!(";base={}", p.base.cxx_name());
                    v += &format!(";type={}", p.value_type.cxx_name());
                    v += &format!(";display={}", p.display.cxx_name());
                    v += &format!(";coord={}", p.coord_type.cxx_name());
                    v += &format!(";group={}", v_str(&p.group));
                    v += &format!(";has_choices={}", v01(p.has_choices()));
                    v += &format!(";choices={}", v_choices(&p.choices()));
                    v += &format!(";hidden_pm={}", v01(p.hide_from_properties_manager));
                    v += &format!(";hidden_re={}", v01(p.hide_from_rules_editor));
                    v += &format!(";hidden_lib={}", v01(p.hide_from_library_editors));
                    v += &format!(";hidden_des={}", v01(p.hide_from_design_editors));
                    v += &format!(";copyable={}", v01(p.copyable));
                    v += &format!(";kind={}", classify_property(p).cxx_name());
                    v += &format!(";lookup={}", v01(pm.get_property(id, p.name) == Some(*pid)));
                    v
                });
            }
        });
    }

    for e in EnumId::ALL {
        let map = e.map();
        o.scope("enummap", e.cxx_name(), |o| {
            emit!(o, "undefined", map.undefined.to_string());
            let choices: Vec<(i32, String)> = map.choices_owned();
            emit!(o, "choices", v_choices(&choices));
        });
    }

    o.text
}

/// The value of one property the way `PCBEXPR_VAR_REF::GetValue` computes it (strings raw,
/// see `dump_props.cpp`).
fn prop_value(p: &Property, it: ItemRef<'_>) -> String {
    let v = p.get(it);

    match (classify_property(p), v) {
        (PropertyKind::IntKind, PropertyValue::Int(i)) => format!("n:{}", v_f64(f64::from(i))),
        (PropertyKind::OptionalInt, PropertyValue::OptInt(i)) => match i {
            Some(i) => format!("n:{}", v_f64(f64::from(i))),
            None => "null".to_string(),
        },
        (PropertyKind::Unsigned, PropertyValue::Unsigned(u)) => {
            format!("n:{}", v_f64(f64::from(u)))
        }
        (PropertyKind::LongLong, PropertyValue::LongLong(l)) => format!("n:{}", v_f64(l as f64)),
        (PropertyKind::Double, PropertyValue::Double(d)) => format!("n:{}", v_f64(d)),
        (PropertyKind::OptionalDouble, PropertyValue::OptDouble(d)) => match d {
            Some(d) => format!("n:{}", v_f64(d)),
            None => "null".to_string(),
        },
        (PropertyKind::BoolKind, PropertyValue::Bool(b)) => {
            format!("n:{}", v_f64(if b { 1.0 } else { 0.0 }))
        }
        (PropertyKind::String, PropertyValue::String(s)) => format!("s:{}", v_str(&s)),
        (PropertyKind::Enum, v @ PropertyValue::Enum(id, value)) => {
            if matches!(p.name, "Layer" | "Layer Top" | "Layer Bottom") && id == EnumId::PcbLayerId
            {
                return format!("layer:{}", v_layer(to_layer_id(value)));
            }

            match v.enum_as_string() {
                Some(s) => format!("s:{}", v_str(s)),
                None => "undef".to_string(),
            }
        }
        (PropertyKind::Angle, PropertyValue::Angle(a)) => format!("n:{}", v_f64(a.as_degrees())),
        (PropertyKind::Color, PropertyValue::Color(c)) => {
            format!("s:{}", v_str(&c.to_css_string()))
        }
        (PropertyKind::Unsupported, _) => "undef".to_string(),
        (k, v) => panic!(
            "property {:?}: value {v:?} does not match kind {k:?}",
            p.name
        ),
    }
}

/// `DumpProps`: the `props` section of one item.
pub(crate) fn dump_props(o: &mut Out, it: ItemRef<'_>) {
    let pm = property_manager();
    let Some(class) = class_of(it) else {
        return;
    };

    emit!(o, "class", class.cxx_name().to_string());

    for pid in pm.properties(class) {
        let p = pm.property(pid);
        o.emit(p.name, || {
            if !pm.is_available_for(class, pid, it) {
                "n/a".to_string()
            } else {
                let mut v = prop_value(p, it);

                if !pm.is_writeable_for(class, pid, it) {
                    v += " (ro)";
                }

                v
            }
        });
    }
}
