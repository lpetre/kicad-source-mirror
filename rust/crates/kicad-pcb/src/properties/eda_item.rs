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

//! Port of `EDA_ITEM_DESC` (`common/eda_item.cpp`): the `Type` property. The `KICAD_T` map
//! itself is in [`super::enums`].

use super::{ClassId, EnumId, Property, PropertyManager, PropertyValue, ValueType};

/// `EDA_ITEM_DESC::EDA_ITEM_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    pm.register_type(ClassId::EdaItem);

    pm.add_property(
        Property::new_enum(
            "Type",
            ClassId::EdaItem,
            ValueType::Enum(EnumId::KicadT),
            false,
            |it| PropertyValue::Enum(EnumId::KicadT, it.type_() as i32),
        ),
        "",
    )
    .set_is_hidden_from_properties_manager();
}
