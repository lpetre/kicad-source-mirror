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
//
// Original file: Copyright (C) 2016 Jean-Pierre Charras, jp.charras at wanadoo.fr

//! Port of `include/gbr_metadata.h` and `common/gbr_metadata.cpp`: special data (item
//! attributes) handled during plot, used by the Gerber plotter to generate auxiliary data
//! (for instance info associated to apertures and flashed pads).
//!
//! Strings: the C++ works on `wxString`, which on the reference platform (Linux) stores
//! UTF-32 code units, i.e. one `wxChar` per Unicode code point. A Rust `char` is the same
//! code point, so every per-`wxChar` loop iterates `str::chars()`. Conversions to
//! `std::string` go through `utf8_str()` and a `const char*`, which stops at the first NUL
//! character; that truncation is reproduced.
//!
//! Out of scope: `FormatStringFromGerber` (Gerber reader) and `GBR_CMP_PNP_METADATA`
//! (placement files). `GBR_APERTURE_METADATA::GetAttributeName` is declared in the header but
//! never defined in C++ and has no port.

use std::fmt::Write as _;

use crate::gbr_netlist_metadata::{GbrNetlistMetadata, gbr_netinfo};

/// Port of `enum GBR_NC_STRING_FORMAT`: options for string format in some attribute strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GbrNcStringFormat {
    /// `GBR_NC_STRING_FORMAT_X1`
    X1,
    /// `GBR_NC_STRING_FORMAT_X2`
    X2,
    /// `GBR_NC_STRING_FORMAT_GBRJOB`
    Gbrjob,
    /// `GBR_NC_STRING_FORMAT_NCDRILL`
    Ncdrill,
}

/// The local date and time of file creation, supplied by the caller.
///
/// The C++ reads the clock (`wxDateTime::GetTimeNow()`) and the local time zone; the port
/// takes them as an explicit parameter (no clock access in library crates).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct GbrCreationDate {
    /// year, e.g. 2018
    pub year: i32,
    /// month 1..12
    pub month: u32,
    /// day of month 1..31
    pub day: u32,
    /// hour 0..23
    pub hour: u32,
    /// minute 0..59
    pub minute: u32,
    /// second 0..59
    pub second: u32,
    /// offset of the local time zone from UTC in minutes (east positive), as `%z` reports it
    pub utc_offset_minutes: i32,
}

impl GbrCreationDate {
    /// `wxDateTime::FormatISOCombined()`: `YYYY-MM-DDTHH:MM:SS`.
    pub fn format_iso_combined(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// `wxDateTime::Format( "%z" )`: `+hhmm` or `-hhmm`.
    pub fn format_tz(&self) -> String {
        let sign = if self.utc_offset_minutes < 0 {
            '-'
        } else {
            '+'
        };
        let abs = self.utc_offset_minutes.unsigned_abs();
        format!("{}{:02}{:02}", sign, abs / 60, abs % 60)
    }
}

/// Port of `GbrMakeCreationDateAttributeString`: create a gerber `TF.CreationDate` attribute.
///
/// The attribute value must conform to the full version of the ISO 8601 date and time format,
/// including time and time zone. Note that this is the date the Gerber file is effectively
/// created, not the time the project PCB was started. Examples:
/// `G04 #@! TF.CreationDate,2018-11-21T08:49:16+01:00*` (X1),
/// `; #@! TF.CreationDate,2018-11-21T08:49:16+01:00` (NC drill),
/// `%TF.CreationDate,2018-11-06T08:25:24+01:00*%` (X2).
///
/// `a_date` replaces the C++ call to the system clock.
pub fn gbr_make_creation_date_attribute_string(
    a_format: GbrNcStringFormat,
    a_date: &GbrCreationDate,
) -> String {
    // The time zone offset format is +mm or +hhmm (or -mm or -hhmm)
    // (mm = number of minutes, hh = number of hours. 1h00mn is returned as +0100)
    // we want +(or -) hh:mm
    let mut timezone_offset = a_date.format_tz();

    if timezone_offset.chars().count() > 3 {
        // format +hhmm or -hhmm found: add separator between hours and minutes
        timezone_offset.insert(3, ':');
    }

    let iso = a_date.format_iso_combined();

    match a_format {
        GbrNcStringFormat::X2 => format!("%TF.CreationDate,{iso}{timezone_offset}*%"),
        GbrNcStringFormat::X1 => format!("G04 #@! TF.CreationDate,{iso}{timezone_offset}*"),
        GbrNcStringFormat::Gbrjob => format!("{iso}{timezone_offset}"),
        GbrNcStringFormat::Ncdrill => format!("; #@! TF.CreationDate,{iso}{timezone_offset}"),
    }
}

/// Port of `GbrMakeProjectGUIDfromString`: build a project GUID using format RFC4122
/// Version 1 or 4 from the project name, because a KiCad project has no specific GUID.
///
/// RFC4122 is used mainly for its syntax, because fields have no meaning for Gerber files
/// and therefore the GUID generated has no meaning because it does not use any time and time
/// stamp specific to the project, just a random pattern (random is here a pattern specific
/// to a project). See en.wikipedia.org/wiki/Universally_unique_identifier
pub fn gbr_make_project_guid_from_string(a_text: &str) -> String {
    /* Gerber GUID format should be RFC4122 Version 1 or 4.
     * The format is:
     * xxxxxxxx-xxxx-Mxxx-Nxxx-xxxxxxxxxxxx
     * with
     *   x = hexDigit lower/upper case
     * and
     *  M = '1' or '4' (UUID version: 1 (basic) or 4 (random)) (we use 4: UUID random)
     * and
     *  N = '8' or '9' or 'A|a' or 'B|b' : UUID variant 1: 2 MSB bits have meaning) (we use N = 9)
     *  N = 1000 or 1001 or 1010 or 1011  : 10xx means Variant 1 (Variant2: 110x and 111x are
     *  reserved)
     */
    let mut guid = String::new();

    // Build a 32 digits GUID from the board name:
    // guid has 32 digits, so add chars in name to be sure we can build a 32 digits guid
    // (i.e. from a 16 char string name)
    // In fact only 30 digits are used, and 2 UID id
    let mut bname: Vec<i32> = a_text.chars().map(|c| c as i32).collect();
    let cnt = 16 - bname.len() as i64;

    if cnt > 0 {
        bname.extend(std::iter::repeat_n('X' as i32, cnt as usize));
    }

    let mut chr_idx = 0usize;

    // Output the 8 first hex digits:
    for _ in 0..4 {
        let cc = bname[chr_idx] & 0xFF;
        chr_idx += 1;
        let _ = write!(guid, "{cc:02x}");
    }

    // Output the 4 next hex digits:
    guid.push('-');

    for _ in 0..2 {
        let cc = bname[chr_idx] & 0xFF;
        chr_idx += 1;
        let _ = write!(guid, "{cc:02x}");
    }

    // Output the 4 next hex digits (UUID version and 3 digits):
    guid.push_str("-4"); // first digit: UUID version 4 (M = 4)
    {
        let mut cc = (bname[chr_idx] << 4) & 0xFF0;
        chr_idx += 1;
        cc += (bname[chr_idx] >> 4) & 0x0F;
        let _ = write!(guid, "{cc:03x}");
    }

    // Output the 4 next hex digits (UUID variant and 3 digits):
    guid.push_str("-9"); // first digit: UUID variant 1 (N = 9)
    {
        let mut cc = (bname[chr_idx] & 0x0F) << 8;
        chr_idx += 1;
        cc += bname[chr_idx] & 0xFF;
        chr_idx += 1;
        let _ = write!(guid, "{cc:03x}");
    }

    // Output the 12 last hex digits:
    guid.push('-');

    for _ in 0..6 {
        let cc = bname[chr_idx] & 0xFF;
        chr_idx += 1;
        let _ = write!(guid, "{cc:02x}");
    }

    guid
}

/// Port of `GBR_APERTURE_METADATA::GBR_APERTURE_ATTRIB`.
///
/// The integer values (declaration order) are stored by `APERTURE::m_ApertureAttribute`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum GbrApertureAttrib {
    /// uninitialized attribute.
    #[default]
    None = 0,
    /// Aperture used for etched components.
    Etchedcmp,
    /// Aperture used for connected items like tracks (not vias).
    Conductor,
    /// Aperture used for board cutout,
    Edgecut,
    /// Aperture used for not connected items (texts, outlines on copper).
    Nonconductor,
    /// Aperture used for vias.
    Viapad,
    /// Aperture used for through hole component on outer layer.
    Componentpad,
    /// Aperture used for SMD pad. Excluded BGA pads which have their own type.
    SmdpadSmdef,
    /// Aperture used for SMD pad with a solder mask defined by the solder mask.
    SmdpadCudef,
    /// Aperture used for BGA pads with a solder mask defined by the copper shape.
    BgapadSmdef,
    /// Aperture used for BGA pad with a solder mask defined by the solder mask.
    BgapadCudef,
    /// Aperture used for edge connector pad (outer layers).
    Connectorpad,
    /// Aperture used for mechanical pads (NPTH).
    Washerpad,
    /// Aperture used for test point pad (outer layers).
    Testpoint,
    /// Aperture used for fiducial pad (outer layers), at board level.
    FiducialGlbl,
    /// Aperture used for fiducial pad (outer layers), at footprint level.
    FiducialLocal,
    /// Aperture used for heat sink pad (typically for SMDs).
    Heatsinkpad,
    /// Aperture used for castellated pads in copper layer files.
    Castellatedpad,
    /// Aperture used for castellated pads in drill files.
    Castellateddrill,
    /// Aperture used for pressfit pads in drill files. This is similar to
    /// `Componentpad` with optional PressFit field.
    Pressfitdrill,
    /// Aperture used for via holes in drill files.
    Viadrill,
    /// Aperture used for backdrill holes in drill files.
    Backdrill,
    /// Aperture used for pad holes in drill files.
    CmpDrill,
    /// Aperture used for pads oblong holes in drill files.
    CmpOblongDrill,
    /// Aperture used for flashed cmp position in placement files.
    CmpPosition,
    /// Aperture used for flashed pin 1 (or A1 or AA1) position in placement files.
    Pad1Pos,
    /// Aperture used for flashed pads position in placement files.
    PadotherPos,
    /// Aperture used to draw component physical body outline without pins in placement files.
    CmpBody,
    /// Aperture used to draw component physical body outline with pins in placement files.
    CmpLead2lead,
    /// Aperture used to draw component footprint bounding box in placement files.
    CmpFootprint,
    /// Aperture used to draw component outline courtyard in placement files.
    CmpCourtyard,
    /// aperture used for other purposes. Requires a text description of this feature.
    Other,
    /// sentinel: max value
    End,
}

impl GbrApertureAttrib {
    /// All values in declaration order (`GBR_APERTURE_ATTRIB_NONE` .. `GBR_APERTURE_ATTRIB_END`).
    pub const ALL: [GbrApertureAttrib; 33] = [
        Self::None,
        Self::Etchedcmp,
        Self::Conductor,
        Self::Edgecut,
        Self::Nonconductor,
        Self::Viapad,
        Self::Componentpad,
        Self::SmdpadSmdef,
        Self::SmdpadCudef,
        Self::BgapadSmdef,
        Self::BgapadCudef,
        Self::Connectorpad,
        Self::Washerpad,
        Self::Testpoint,
        Self::FiducialGlbl,
        Self::FiducialLocal,
        Self::Heatsinkpad,
        Self::Castellatedpad,
        Self::Castellateddrill,
        Self::Pressfitdrill,
        Self::Viadrill,
        Self::Backdrill,
        Self::CmpDrill,
        Self::CmpOblongDrill,
        Self::CmpPosition,
        Self::Pad1Pos,
        Self::PadotherPos,
        Self::CmpBody,
        Self::CmpLead2lead,
        Self::CmpFootprint,
        Self::CmpCourtyard,
        Self::Other,
        Self::End,
    ];

    /// The C++ integer value.
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// The value of a C++ integer; `None` when out of range.
    pub fn from_i32(value: i32) -> Option<GbrApertureAttrib> {
        usize::try_from(value)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }
}

/// Port of `GBR_APERTURE_METADATA`: info which can be added in a gerber file as attribute of
/// an aperture, by the `%TA.AperFunction` command.
///
/// This attribute is added when creating a new aperture (command `%ADDxx`). Only one aperture
/// attribute can be added to a given aperture.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GbrApertureMetadata {
    /// The id of the aperture attribute
    pub apert_attribute: GbrApertureAttrib,
    /// the text of a `GBR_APERTURE_ATTRIB_OTHER` attribute
    pub custom_attribute: String,
}

impl GbrApertureMetadata {
    /// Port of `GBR_APERTURE_METADATA::GBR_APERTURE_METADATA()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of the static `GBR_APERTURE_METADATA::FormatAttribute`: the full command string
    /// corresponding to the aperture attribute like `%TA.AperFunction,<function>*%` followed
    /// by a newline.
    ///
    /// `a_use_x1_structured_comment` is false in X2 mode and true in X1 mode to add the
    /// attribute inside a compatible X1 structured comment starting by `G04 #@! `.
    pub fn format_attribute_static(
        a_attribute: GbrApertureAttrib,
        a_use_x1_structured_comment: bool,
        a_custom_attribute: &str,
    ) -> String {
        use GbrApertureAttrib as A;

        // the specific aperture attribute (TA.xxx)
        let mut attribute_string = String::new();
        // a optional G04 comment line to write before the TA. line
        let mut comment_string = "";

        // generate a string to print a Gerber Aperture attribute
        match a_attribute {
            // Dummy value (aAttribute must be < GBR_APERTURE_ATTRIB_END).
            A::End | A::None => {} // idle command: do nothing
            // print info associated to an item which connects 2 different nets
            // (Net tees, microwave component)
            A::Etchedcmp => attribute_string = "TA.AperFunction,EtchedComponent".into(),
            // print info associated to a track
            A::Conductor => attribute_string = "TA.AperFunction,Conductor".into(),
            // print info associated to a board outline
            A::Edgecut => attribute_string = "TA.AperFunction,Profile".into(),
            // print info associated to a flashed via
            A::Viapad => attribute_string = "TA.AperFunction,ViaPad".into(),
            // print info associated to a item on a copper layer which is not a track (for
            // instance a text)
            A::Nonconductor => attribute_string = "TA.AperFunction,NonConductor".into(),
            // print info associated to a flashed through hole component on outer layer
            A::Componentpad => attribute_string = "TA.AperFunction,ComponentPad".into(),
            // print info associated to a flashed for SMD pad with solder mask defined from
            // the copper shape. Excluded BGA pads which have their own type
            A::SmdpadSmdef => attribute_string = "TA.AperFunction,SMDPad,SMDef".into(),
            // print info associated to a flashed SMD pad with a solder mask defined by the
            // solder mask
            A::SmdpadCudef => attribute_string = "TA.AperFunction,SMDPad,CuDef".into(),
            // print info associated to flashed BGA pads with a solder mask defined by the
            // copper shape
            A::BgapadSmdef => attribute_string = "TA.AperFunction,BGAPad,SMDef".into(),
            // print info associated to a flashed BGA pad with a solder mask defined by the
            // solder mask
            A::BgapadCudef => attribute_string = "TA.AperFunction,BGAPad,CuDef".into(),
            // print info associated to a flashed edge connector pad (outer layers)
            A::Connectorpad => attribute_string = "TA.AperFunction,ConnectorPad".into(),
            // print info associated to flashed mechanical pads (NPTH)
            A::Washerpad => attribute_string = "TA.AperFunction,WasherPad".into(),
            // print info associated to a flashed heat sink pad (typically for SMDs)
            A::Heatsinkpad => attribute_string = "TA.AperFunction,HeatsinkPad".into(),
            // print info associated to a flashed test point pad (typically for SMDs)
            A::Testpoint => attribute_string = "TA.AperFunction,TestPad".into(),
            // print info associated to a flashed fiducial pad (typically for SMDs)
            A::FiducialGlbl => attribute_string = "TA.AperFunction,FiducialPad,Global".into(),
            // print info associated to a flashed fiducial pad (typically for SMDs)
            A::FiducialLocal => attribute_string = "TA.AperFunction,FiducialPad,Local".into(),
            // print info associated to a flashed castellated pad (typically for SMDs)
            A::Castellatedpad => attribute_string = "TA.AperFunction,CastellatedPad".into(),
            // print info associated to a flashed castellated pad in drill files
            A::Castellateddrill => attribute_string = "TA.AperFunction,CastellatedDrill".into(),
            // print info associated to a via hole in drill files
            A::Viadrill => attribute_string = "TA.AperFunction,ViaDrill".into(),
            A::Backdrill => attribute_string = "TA.AperFunction,BackDrill".into(),
            // print info associated to a component round pad hole in drill files
            A::CmpDrill => attribute_string = "TA.AperFunction,ComponentDrill".into(),
            // print info associated to a flashed component pad with pressfit option in drill
            // files
            A::Pressfitdrill => attribute_string = "TA.AperFunction,ComponentDrill,PressFit".into(),
            // print info associated to a component oblong pad hole in drill files.
            // Same as a round pad hole, but is a specific aperture in drill file and
            // a G04 comment is added to the aperture function
            A::CmpOblongDrill => {
                comment_string = "aperture for slot hole";
                attribute_string = "TA.AperFunction,ComponentDrill".into();
            }
            // print info associated to a component flashed shape at the component position
            // in placement files
            A::CmpPosition => attribute_string = "TA.AperFunction,ComponentMain".into(),
            // print info associated to a component flashed shape at pad 1 position
            // (pad 1 is also pad A1 or pad AA1) in placement files
            A::Pad1Pos => attribute_string = "TA.AperFunction,ComponentPin".into(),
            // print info associated to a component flashed shape at pads position (all but
            // pad 1) in placement files. Currently: (could be changed later) same as
            // GBR_APERTURE_ATTRIB_PADOTHER_POS
            A::PadotherPos => attribute_string = "TA.AperFunction,ComponentPin".into(),
            // print the component physical body polygon in placement files
            A::CmpBody => attribute_string = "TA.AperFunction,ComponentOutline,Body".into(),
            // print the component physical lead to lead polygon in placement files
            A::CmpLead2lead => {
                attribute_string = "TA.AperFunction,ComponentOutline,Lead2Lead".into()
            }
            // print the component footprint bounding box polygon in placement files
            A::CmpFootprint => {
                attribute_string = "TA.AperFunction,ComponentOutline,Footprint".into()
            }
            // print the component courtyard polygon in placement files
            A::CmpCourtyard => {
                attribute_string = "TA.AperFunction,ComponentOutline,Courtyard".into()
            }
            A::Other => attribute_string = format!("TA.AperFunction,Other,{a_custom_attribute}"),
        }

        let mut full_attribute_string = String::new();
        let mut eol_string = "";

        if !attribute_string.is_empty() {
            if !comment_string.is_empty() {
                full_attribute_string = format!("G04 {comment_string}*\n");
            }

            if a_use_x1_structured_comment {
                full_attribute_string += "G04 #@! ";
                eol_string = "*\n";
            } else {
                full_attribute_string += "%";
                eol_string = "*%\n";
            }
        }

        full_attribute_string += &attribute_string;
        full_attribute_string += eol_string;
        full_attribute_string
    }

    /// Port of the member `GBR_APERTURE_METADATA::FormatAttribute( bool )`.
    pub fn format_attribute(&self, a_use_x1_structured_comment: bool) -> String {
        Self::format_attribute_static(
            self.apert_attribute,
            a_use_x1_structured_comment,
            &self.custom_attribute,
        )
    }
}

/// Port of `GBR_METADATA`: metadata which can be added in a gerber file as attribute in X2
/// format.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GbrMetadata {
    /// An item to handle aperture attribute.
    pub aperture_metadata: GbrApertureMetadata,
    /// An item to handle object attribute.
    pub netlist_metadata: GbrNetlistMetadata,
    /// If the metadata is relative to a copper layer or not, this flag which can be set/reset
    /// when an attribute for a given item depends on whether a copper layer or a non copper
    /// layer is plotted. The initial state is false.
    is_copper: bool,
}

impl GbrMetadata {
    /// Port of `GBR_METADATA::GBR_METADATA()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `GBR_METADATA::SetApertureAttrib( GBR_APERTURE_ATTRIB )`.
    pub fn set_aperture_attrib(&mut self, a_apert_attribute: GbrApertureAttrib) {
        self.aperture_metadata.apert_attribute = a_apert_attribute;
    }

    /// Port of `GBR_METADATA::SetApertureAttrib( std::string )`: a custom
    /// (`GBR_APERTURE_ATTRIB_OTHER`) attribute.
    pub fn set_aperture_attrib_custom(&mut self, a_custom_attribute: &str) {
        self.aperture_metadata.apert_attribute = GbrApertureAttrib::Other;
        self.aperture_metadata.custom_attribute = a_custom_attribute.to_string();
    }

    /// Port of `GBR_METADATA::GetApertureAttrib()`.
    pub fn aperture_attrib(&self) -> GbrApertureAttrib {
        self.aperture_metadata.apert_attribute
    }

    /// Port of `GBR_METADATA::GetCustomAttribute()`.
    pub fn custom_attribute(&self) -> &str {
        &self.aperture_metadata.custom_attribute
    }

    /// Port of `GBR_METADATA::SetNetAttribType()`.
    pub fn set_net_attrib_type(&mut self, a_net_attrib_type: i32) {
        self.netlist_metadata.net_attrib_type = a_net_attrib_type;
    }

    /// Port of `GBR_METADATA::GetNetAttribType()`.
    pub fn net_attrib_type(&self) -> i32 {
        self.netlist_metadata.net_attrib_type
    }

    /// Port of `GBR_METADATA::SetNetName()`.
    pub fn set_net_name(&mut self, a_netname: &str) {
        self.netlist_metadata.netname = a_netname.to_string();
    }

    /// Port of `GBR_METADATA::SetPadName()` (C++ defaults: `aUseUTF8 = false`,
    /// `aEscapeString = false`).
    pub fn set_pad_name(&mut self, a_padname: &str, a_use_utf8: bool, a_escape_string: bool) {
        self.netlist_metadata
            .padname
            .set_field(a_padname, a_use_utf8, a_escape_string);
    }

    /// Port of `GBR_METADATA::SetPadPinFunction()`.
    pub fn set_pad_pin_function(
        &mut self,
        a_pad_pin_function: &str,
        a_use_utf8: bool,
        a_escape_string: bool,
    ) {
        self.netlist_metadata.pad_pin_function.set_field(
            a_pad_pin_function,
            a_use_utf8,
            a_escape_string,
        );
    }

    /// Port of `GBR_METADATA::SetCmpReference()`.
    pub fn set_cmp_reference(&mut self, a_component_ref: &str) {
        self.netlist_metadata.cmpref = a_component_ref.to_string();
    }

    /// Port of `GBR_METADATA::IsCopper()`. Allowed attributes are not the same on board
    /// copper layers and on other layers.
    pub fn is_copper(&self) -> bool {
        self.is_copper
    }

    /// Port of `GBR_METADATA::SetCopper()`.
    pub fn set_copper(&mut self, a_value: bool) {
        self.is_copper = a_value;
    }
}

/// `static_cast<const char*>( wxString::utf8_str() )` assigned to a `std::string`: the UTF-8
/// text up to (excluding) the first NUL character.
fn utf8_c_str(a_string: &str) -> String {
    match a_string.find('\0') {
        Some(pos) => a_string[..pos].to_string(),
        None => a_string.to_string(),
    }
}

/// Port of `ConvertNotAllowedCharsInGerber`: normalize `a_string` and convert it to a Gerber
/// compatible string.
///
/// Normalization means convert to a hexadecimal 16 bit sequence Unicode (`\uXXXX`) and on
/// request convert any code > 0x7F. Illegal characters are `,` `*` `%` `\` (and `"` when
/// quoting). `a_allow_utf8_chars` false converts non ASCII7 values to a Unicode sequence;
/// `a_quote_string` true double quotes the returned string.
pub fn convert_not_allowed_chars_in_gerber(
    a_string: &str,
    a_allow_utf8_chars: bool,
    a_quote_string: bool,
) -> String {
    /* format string means convert any code > 0x7E and unauthorized codes to a hexadecimal
     * 16 bits sequence Unicode
     * However if aAllowUtf8Chars is true only unauthorized codes will be escaped, because some
     * Gerber files accept UTF8 chars.
     * unauthorized codes are ',' '*' '%' '\' '"' and are used as separators in Gerber files
     */
    let mut txt = String::new();

    if a_quote_string {
        txt.push('"');
    }

    for code in a_string.chars() {
        let mut convert = match code {
            '\\' | '%' | '*' | ',' => true,
            '"' => a_quote_string,
            _ => false,
        };

        if !a_allow_utf8_chars && (code as u32) > 0x7F {
            convert = true;
        }

        if convert {
            // Convert code to 4 hexadecimal digit
            // (Gerber allows only 4 hexadecimal digit) in escape seq:
            // "\uXXXX", XXXX is the Unicode 16 bits hexa value
            let _ = write!(txt, "\\u{:04X}", (code as u32) & 0xFFFF);
        } else {
            txt.push(code);
        }
    }

    if a_quote_string {
        txt.push('"');
    }

    txt
}

/// Port of `GBR_DATA_FIELD::GetGerberString`.
pub fn gbr_data_field_gerber_string(a_field: &crate::gbr_netlist_metadata::GbrDataField) -> String {
    let mut converted = String::new();

    if !a_field.is_empty() {
        converted = convert_not_allowed_chars_in_gerber(
            a_field.value(),
            a_field.use_utf8(),
            a_field.escape_string(),
        );
    }

    // Convert the char string to std::string. Be careful when converting a wxString to
    // a std::string: using static_cast<const char*> is mandatory
    utf8_c_str(&converted)
}

impl crate::gbr_netlist_metadata::GbrDataField {
    /// Port of `GBR_DATA_FIELD::GetGerberString`.
    pub fn gerber_string(&self) -> String {
        gbr_data_field_gerber_string(self)
    }
}

/// Port of `FormatStringToGerber`: normalize `a_string` and convert it to a Gerber string.
///
/// Normalization means convert any code > 0x7F and unauthorized code to a hexadecimal 16 bit
/// sequence Unicode. Illegal characters are `,` `*` `%` `\`. The result is an ASCII7 coded
/// compliant gerber string (except for quoted strings, see below).
pub fn format_string_to_gerber(a_string: &str) -> String {
    /* format string means convert any code > 0x7E and unauthorized codes to a hexadecimal
     * 16 bits sequence Unicode
     * unauthorized codes are ',' '*' '%' '\'
     * This conversion is not made for quoted strings, because if the string is
     * quoted, the conversion is expected to be already made, and the returned string must use
     * UTF8 encoding
     */
    let converted =
        if !a_string.is_empty() && (!a_string.starts_with('"') || !a_string.ends_with('"')) {
            convert_not_allowed_chars_in_gerber(a_string, false, false)
        } else {
            a_string.to_string()
        };

    // Convert the char string to std::string. Be careful when converting a wxString to
    // a std::string: using static_cast<const char*> is mandatory
    utf8_c_str(&converted)
}

/// Netname and Pad num fields cannot be empty in Gerber files.
/// Normalized names must be used, if any.
/// Net name of not connected pads (one pad net) (normalized).
const NO_NET_NAME: &str = "N/C";
/// Pad name of pads without pad name/number (not normalized).
const NO_PAD_NAME: &str = "";

/// The outputs of [`format_net_attribute`] when it returns `true` in C++.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetAttributeText {
    /// `aPrintedText`: the string to print; `None` when the attributes did not change (the C++
    /// leaves the caller's string untouched then)
    pub printed_text: Option<String>,
    /// `aClearPreviousAttributes`: true if the full set of attributes must be deleted from
    /// file before adding new attribute (happens when a previous attribute no longer exists)
    pub clear_previous_attributes: bool,
}

/// Port of `FormatNetAttribute`: generate the string to set a net attribute for a graphic
/// object to print to a gerber file.
///
/// `a_last_net_attributes` is the current full set of attributes (updated). `a_data` is the
/// metadata associated to the graphic object. Returns `None` (C++ `false`) if nothing can be
/// done (no metadata, or `GBR_NETINFO_UNSPECIFIED`); then `aClearPreviousAttributes` is false.
/// `a_use_x1_structured_comment` is false in X2 mode and true in X1 mode to add the net
/// attribute in compatible X1 structured comment (i.e. prefixed by `G04 #@! `).
pub fn format_net_attribute(
    a_last_net_attributes: &mut String,
    a_data: Option<&GbrNetlistMetadata>,
    a_use_x1_structured_comment: bool,
) -> Option<NetAttributeText> {
    let (prepend_string, eol_string) = if a_use_x1_structured_comment {
        ("G04 #@! ", "*\n")
    } else {
        ("%", "*%\n")
    };

    // print a Gerber net attribute record.
    // it is added to the object attributes dictionary
    // On file, only modified or new attributes are printed.
    let a_data = a_data?;

    let mut pad_attribute_string = String::new();
    let mut net_attribute_string = String::new();
    let mut cmp_attribute_string = String::new();

    if a_data.net_attrib_type == gbr_netinfo::UNSPECIFIED {
        return None; // idle command: do nothing
    }

    if (a_data.net_attrib_type & gbr_netinfo::PAD) != 0 {
        // print info associated to a flashed pad (cmpref, pad name, and optionally pin
        // function)
        // example1: %TO.P,R5,3*%
        // example2: %TO.P,R5,3,reset*%
        pad_attribute_string = format!("{prepend_string}TO.P,");
        pad_attribute_string += &format_string_to_gerber(&a_data.cmpref);
        pad_attribute_string.push(',');

        if a_data.padname.is_empty() {
            // Happens for "mechanical" or never connected pads
            pad_attribute_string += &format_string_to_gerber(NO_PAD_NAME);
        } else {
            pad_attribute_string += &a_data.padname.gerber_string();

            // In Pcbnew, the pin function comes from the schematic.
            // so it exists only for named pads
            if !a_data.pad_pin_function.is_empty() {
                pad_attribute_string.push(',');
                pad_attribute_string += &a_data.pad_pin_function.gerber_string();
            }
        }

        pad_attribute_string += eol_string;
    }

    if (a_data.net_attrib_type & gbr_netinfo::NET) != 0 {
        // print info associated to a net
        // example: %TO.N,Clk3*%
        net_attribute_string = format!("{prepend_string}TO.N,");

        if a_data.netname.is_empty() {
            if a_data.not_in_net {
                // Happens for not connectable pads: mechanical pads
                // and pads with no padname/num
                // In this case the net name must be left empty
            } else {
                // Happens for not connected pads: use a normalized
                // dummy name
                net_attribute_string += &format_string_to_gerber(NO_NET_NAME);
            }
        } else {
            net_attribute_string += &format_string_to_gerber(&a_data.netname);
        }

        net_attribute_string += eol_string;
    }

    if (a_data.net_attrib_type & gbr_netinfo::CMP) != 0
        && (a_data.net_attrib_type & gbr_netinfo::PAD) == 0
    {
        // print info associated to a footprint
        // example: %TO.C,R2*%
        // Because GBR_NETINFO_PAD option already contains this info, it is not
        // created here for a GBR_NETINFO_PAD attribute
        cmp_attribute_string = format!("{prepend_string}TO.C,");
        cmp_attribute_string += &format_string_to_gerber(&a_data.cmpref);
        cmp_attribute_string += eol_string;
    }

    // the full list of requested attributes:
    let full_attribute_string =
        format!("{pad_attribute_string}{net_attribute_string}{cmp_attribute_string}");
    // the short list of requested attributes
    // (only modified or new attributes are stored here):
    let mut short_attribute_string = String::new();

    let mut result = NetAttributeText::default();

    // Attributes have changed: update attribute string, and see if the previous attribute
    // list (dictionary in Gerber language) must be cleared
    if *a_last_net_attributes != full_attribute_string {
        // first, remove no longer existing attributes.
        // Because in KiCad the full attribute list is evaluated for each object,
        // the entire dictionary is cleared
        // If m_TryKeepPreviousAttributes is true, only the no longer existing attribute
        // is cleared.
        // Note: to avoid interaction between clear attributes and set attributes
        // the clear attribute is inserted first.
        let mut clear_dict = false;
        let last = a_last_net_attributes.as_str();

        if last.contains("TO.P,") {
            if pad_attribute_string.is_empty() {
                // No more this attribute
                if a_data.try_keep_previous_attributes {
                    // Clear only this attribute
                    short_attribute_string
                        .insert_str(0, &format!("{prepend_string}TO.P{eol_string}"));
                } else {
                    clear_dict = true;
                }
            } else if !last.contains(&pad_attribute_string) {
                // This attribute has changed
                short_attribute_string += &pad_attribute_string;
            }
        } else {
            // New attribute
            short_attribute_string += &pad_attribute_string;
        }

        if last.contains("TO.N,") {
            if net_attribute_string.is_empty() {
                // No more this attribute
                if a_data.try_keep_previous_attributes {
                    // Clear only this attribute
                    short_attribute_string
                        .insert_str(0, &format!("{prepend_string}TO.N{eol_string}"));
                } else {
                    clear_dict = true;
                }
            } else if !last.contains(&net_attribute_string) {
                // This attribute has changed.
                short_attribute_string += &net_attribute_string;
            }
        } else {
            // New attribute
            short_attribute_string += &net_attribute_string;
        }

        if last.contains("TO.C,") {
            if cmp_attribute_string.is_empty() {
                // No more this attribute
                if a_data.try_keep_previous_attributes {
                    // Clear only this attribute
                    // Refinement:
                    // the attribute will be cleared only if there is no pad attribute.
                    // If a pad attribute exists, the component name exists so the old
                    // TO.C value will be updated, therefore no need to clear it before updating
                    if pad_attribute_string.is_empty() {
                        short_attribute_string
                            .insert_str(0, &format!("{prepend_string}TO.C{eol_string}"));
                    }
                } else {
                    clear_dict = true;
                }
            } else if !last.contains(&cmp_attribute_string) {
                // This attribute has changed.
                short_attribute_string += &cmp_attribute_string;
            }
        } else {
            // New attribute
            short_attribute_string += &cmp_attribute_string;
        }

        result.clear_previous_attributes = clear_dict;

        *a_last_net_attributes = full_attribute_string.clone();

        result.printed_text = Some(if clear_dict {
            full_attribute_string
        } else {
            short_attribute_string
        });
    }

    Some(result)
}
