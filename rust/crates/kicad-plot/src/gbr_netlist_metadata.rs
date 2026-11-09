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

//! Port of `include/gbr_netlist_metadata.h`: `GBR_DATA_FIELD` and `GBR_NETLIST_METADATA`
//! (Gerber object attributes `%TO.P`, `%TO.N`, `%TO.C`).
//!
//! `GBR_CMP_PNP_METADATA` (placement files) is out of scope, see `PORT_STATUS.md`.
//! `GBR_DATA_FIELD::GetGerberString` is implemented in `gbr_metadata.cpp` and ported in
//! [`crate::gbr_metadata`].

/// Port of `GBR_DATA_FIELD`: a Gerber data field.
///
/// This is a Unicode string with some chars converted in escaped hexadecimal sequence when
/// creating the file. The following characters are always escaped because they are separator
/// in Gerber files: `*` `,` `\` `%`. Non ASCII7 characters can be converted to UTF8 or escaped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GbrDataField {
    /// the Unicode text to print in Gbr file (after escape and quoting)
    field: String,
    /// true to use UTF8, false to escape non ASCII7 chars
    use_utf8: bool,
    /// true to quote the field in gbr file
    escape_string: bool,
}

impl GbrDataField {
    /// Port of `GBR_DATA_FIELD::GBR_DATA_FIELD()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `GBR_DATA_FIELD::clear()` / `Clear()`.
    pub fn clear(&mut self) {
        self.field.clear();
        self.use_utf8 = false;
        self.escape_string = false;
    }

    /// Port of `GBR_DATA_FIELD::GetValue()`.
    pub fn value(&self) -> &str {
        &self.field
    }

    /// Port of `GBR_DATA_FIELD::SetField()`.
    pub fn set_field(&mut self, a_field: &str, a_use_utf8: bool, a_escape_string: bool) {
        self.field = a_field.to_string();
        self.use_utf8 = a_use_utf8;
        self.escape_string = a_escape_string;
    }

    /// Port of `GBR_DATA_FIELD::IsEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.field.is_empty()
    }

    /// `m_useUTF8`.
    pub fn use_utf8(&self) -> bool {
        self.use_utf8
    }

    /// `m_escapeString`.
    pub fn escape_string(&self) -> bool {
        self.escape_string
    }
}

/// Port of `GBR_NETLIST_METADATA::GBR_NETINFO_TYPE`. The values can be OR'ed for items which
/// can have more than one attribute (a flashed pad has all allowed attributes).
pub mod gbr_netinfo {
    /// `GBR_NETINFO_UNSPECIFIED`: idle command (no command)
    pub const UNSPECIFIED: i32 = 0;
    /// `GBR_NETINFO_PAD`: print info associated to a flashed pad (TO.P attribute)
    pub const PAD: i32 = 1;
    /// `GBR_NETINFO_NET`: print info associated to a net (TO.N attribute)
    pub const NET: i32 = 2;
    /// `GBR_NETINFO_CMP`: print info associated to a component (TO.C attribute)
    pub const CMP: i32 = 4;
    /// `GBR_NETINFO_ALL`: flashed pads use the full attribute set
    pub const ALL: i32 = PAD | NET | CMP;
}

/// Port of `GBR_NETLIST_METADATA`: information which can be added in a gerber file as
/// attribute of an object.
///
/// The `GBR_NETINFO_TYPE` types can be OR'ed to add 2 (or more) attributes. There are only 3
/// net attributes defined attached to an object by the `%TO` command: `%TO.P`, `%TO.N`,
/// `%TO.C`. The .P attribute can be used only for flashed pads (using the D03 command) and
/// only for external copper layers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GbrNetlistMetadata {
    /// the type of net info (used to define the gerber string to create), a bit set of
    /// [`gbr_netinfo`] values
    pub net_attrib_type: i32,
    /// true if a pad of a footprint cannot be connected (for instance a mechanical NPTH, or
    /// a not named pad); in this case the pad net name is empty in gerber file
    pub not_in_net: bool,
    /// for a flashed pad: the pad name (TO.P attribute)
    pub padname: GbrDataField,
    /// for a pad: the pin function (defined in schematic)
    pub pad_pin_function: GbrDataField,
    /// the component reference parent of the data
    pub cmpref: String,
    /// for items associated to a net: the netname
    pub netname: String,
    /// a string to print after %TO object attributes, if not empty it is printed "as this"
    pub extra_data: String,
    /// If true, do not clear all attributes when a attribute has changed. This is useful
    /// when some attributes need to be persistent. If false, attributes will be cleared if
    /// only one attribute cleared. This is a more secure way to set attributes, when all
    /// attribute changes are not safely managed.
    pub try_keep_previous_attributes: bool,
}

impl GbrNetlistMetadata {
    /// Port of `GBR_NETLIST_METADATA::GBR_NETLIST_METADATA()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `GBR_NETLIST_METADATA::ClearExtraData()`.
    pub fn clear_extra_data(&mut self) {
        self.extra_data.clear();
    }

    /// Port of `GBR_NETLIST_METADATA::SetExtraData()`.
    pub fn set_extra_data(&mut self, a_extra_data: &str) {
        self.extra_data = a_extra_data.to_string();
    }

    /// Port of `GBR_NETLIST_METADATA::ClearAttribute()`: remove the net attribute specified by
    /// `a_name` (`.CN`, `.P`, `.N` or `.C`). If `a_name` is `None` or empty, remove all
    /// attributes.
    pub fn clear_attribute(&mut self, a_name: Option<&str>) {
        if self.net_attrib_type == gbr_netinfo::UNSPECIFIED {
            self.padname.clear();
            self.pad_pin_function.clear();
            self.cmpref.clear();
            self.netname.clear();
            return;
        }

        let name = a_name.unwrap_or("");

        if name.is_empty() || name == ".CN" {
            self.net_attrib_type = gbr_netinfo::UNSPECIFIED;
            self.padname.clear();
            self.pad_pin_function.clear();
            self.cmpref.clear();
            self.netname.clear();
            return;
        }

        if name == ".C" {
            self.net_attrib_type &= !gbr_netinfo::CMP;
            self.cmpref.clear();
            return;
        }

        if name == ".N" {
            self.net_attrib_type &= !gbr_netinfo::NET;
            self.netname.clear();
            return;
        }

        if name == ".P" {
            self.net_attrib_type &= !gbr_netinfo::PAD;
            self.padname.clear();
            self.pad_pin_function.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_attribute() {
        let mut md = GbrNetlistMetadata::new();
        md.net_attrib_type = gbr_netinfo::ALL;
        md.cmpref = "R1".into();
        md.netname = "GND".into();
        md.padname.set_field("1", false, false);

        md.clear_attribute(Some(".C"));
        assert_eq!(md.net_attrib_type, gbr_netinfo::PAD | gbr_netinfo::NET);
        assert!(md.cmpref.is_empty());

        md.clear_attribute(Some(".P"));
        assert_eq!(md.net_attrib_type, gbr_netinfo::NET);
        assert!(md.padname.is_empty());

        md.clear_attribute(None);
        assert_eq!(md.net_attrib_type, gbr_netinfo::UNSPECIFIED);
        assert!(md.netname.is_empty());
    }
}
