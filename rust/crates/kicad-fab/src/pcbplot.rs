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

//! Port of `pcbnew/pcbplot.cpp` and the free declarations of `pcbnew/pcbplot.h`: Protel file
//! extensions, the X2 file function and polarity attributes, `AddGerberX2Header` and
//! `AddGerberX2Attribute`.
//!
//! Work package FAB-03 (`docs/analysis/plot-fab.md` 6.1, 6.3). The `BRDITEMS_PLOTTER` class
//! declared in `pcbplot.h` lives in [`crate::plot_brditems_plotter`]; `StartPlotBoard`,
//! `PlotBoardLayers` and `PlotOneBoardLayer` in [`crate::plot_board_layers`].
//!
//! `GetBuildVersion()` and the wall clock read by `GbrMakeCreationDateAttributeString` are
//! caller-supplied values ([`PlotEnv`]).

use kicad_core::layer_ids::is_copper_layer;
use kicad_core::layer_ids::copper_layer_to_ordinal;
use kicad_core::text_vars::{ResolutionContext, expand_text_vars};
use kicad_core::PcbLayerId;
use kicad_pcb::board::Board;
use kicad_plot::Plotter;
use kicad_plot::gbr_metadata::{
    GbrCreationDate, GbrNcStringFormat, gbr_make_creation_date_attribute_string,
    gbr_make_project_guid_from_string,
};

use crate::wx_file_name::WxFileName;

/// `PLOT_MIN_SCALE`: minimum reasonable value for the plot / print scale.
pub const PLOT_MIN_SCALE: f64 = 0.01;

/// `PLOT_MAX_SCALE`: maximum reasonable value for the plot / print scale.
pub const PLOT_MAX_SCALE: f64 = 100.0;

/// The values C++ reads from the build and from the environment while plotting, supplied by
/// the caller (no clock access in library crates; `docs/analysis/plot-fab.md` 5.6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlotEnv {
    /// `GetBuildVersion()`, e.g. `10.99.0-unknown`.
    pub build_version: String,
    /// `wxDateTime::Now()` and the local UTC offset. One timestamp feeds every date format of
    /// a plot (`TF.CreationDate`, the `G04 Created by` comment, the SVG title).
    pub creation_date: GbrCreationDate,
}

impl PlotEnv {
    /// `wxDateTime::Now().FormatISOCombined( ' ' )` (`YYYY-MM-DD HH:MM:SS`), the date of the
    /// Gerber `G04 Created by KiCad` comment.
    pub fn gerber_comment_date(&self) -> String {
        self.creation_date.format_iso_combined().replacen('T', " ", 1)
    }

    /// `GetISO8601CurrentDateTime()` (`YYYY-MM-DDTHH:MM:SS`), the date of the SVG title.
    pub fn iso8601_date(&self) -> String {
        self.creation_date.format_iso_combined()
    }
}

/// Port of `GetGerberProtelExtension`: the Protel-style file extension of a layer
/// (`gtl`, `gbl`, `g<n>`, `gta`, ..., `gm1`, else `gbr`).
pub fn gerber_protel_extension(a_layer: i32) -> String {
    if is_copper_layer(a_layer) {
        if a_layer == PcbLayerId::FCu.0 {
            "gtl".to_string()
        } else if a_layer == PcbLayerId::BCu.0 {
            "gbl".to_string()
        } else {
            format!("g{}", copper_layer_to_ordinal(PcbLayerId(a_layer)))
        }
    } else {
        match PcbLayerId(a_layer) {
            PcbLayerId::BAdhes => "gba",
            PcbLayerId::FAdhes => "gta",

            PcbLayerId::BPaste => "gbp",
            PcbLayerId::FPaste => "gtp",

            PcbLayerId::BSilkS => "gbo",
            PcbLayerId::FSilkS => "gto",

            PcbLayerId::BMask => "gbs",
            PcbLayerId::FMask => "gts",

            PcbLayerId::EdgeCuts => "gm1",

            // Dwgs_User, Cmts_User, Eco1_User, Eco2_User and everything else
            _ => "gbr",
        }
        .to_string()
    }
}

/// Port of `GetGerberFileFunctionAttribute`: the `%TF.FileFunction,<function>*%` line of a
/// layer (X2 syntax).
pub fn gerber_file_function_attribute(a_board: &Board, a_layer: i32) -> String {
    let attrib: String = match PcbLayerId(a_layer) {
        PcbLayerId::FAdhes => "Glue,Top".to_string(),
        PcbLayerId::BAdhes => "Glue,Bot".to_string(),
        PcbLayerId::FSilkS => "Legend,Top".to_string(),
        PcbLayerId::BSilkS => "Legend,Bot".to_string(),
        PcbLayerId::FMask => "Soldermask,Top".to_string(),
        PcbLayerId::BMask => "Soldermask,Bot".to_string(),
        PcbLayerId::FPaste => "Paste,Top".to_string(),
        PcbLayerId::BPaste => "Paste,Bot".to_string(),

        // Board outline.
        // Can be "Profile,NP" (Not Plated: usual) or "Profile,P"
        // This last is the exception (Plated)
        PcbLayerId::EdgeCuts => "Profile,NP".to_string(),

        PcbLayerId::DwgsUser => "OtherDrawing,Comment".to_string(),
        PcbLayerId::CmtsUser => "Other,Comment".to_string(),
        PcbLayerId::Eco1User => "Other,ECO1".to_string(),
        PcbLayerId::Eco2User => "Other,ECO2".to_string(),

        // This is actually a assembly layer
        PcbLayerId::BFab => "AssemblyDrawing,Bot".to_string(),
        // This is actually a assembly layer
        PcbLayerId::FFab => "AssemblyDrawing,Top".to_string(),

        PcbLayerId::BCu => format!("Copper,L{},Bot", a_board.copper_layer_count()),
        PcbLayerId::FCu => "Copper,L1,Top".to_string(),

        _ => {
            if is_copper_layer(a_layer) {
                // aLayer use even values, and the first internal layer
                // is B_Cu + 2. And in gerber file, layer id is 2 (1 is F_Cu)
                let ly_id = ((a_layer - PcbLayerId::BCu.0) / 2) + 1;
                format!("Copper,L{ly_id},Inr")
            } else {
                // C++: attrib.Printf( "Other,User", aLayer + 1 ) (the argument is unused)
                "Other,User".to_string()
            }
        }
    };

    // The C++ keeps a disabled ("#if 0") block here that would append the signal type of
    // copper layers (",Signal", ",Plane", ",Mixed"); it is not used by Pcbnew.

    format!("%TF.FileFunction,{attrib}*%")
}

/// Port of `GetGerberFilePolarityAttribute` (`static`): `%TF.FilePolarity,Positive*%` or
/// `%TF.FilePolarity,Negative*%`, or an empty string for layers which do not use a polarity.
///
/// The value of the .FilePolarity specifies whether the image represents the presence or
/// absence of material. This attribute can only be used when the file represents a pattern in
/// a material layer, e.g. copper, solder mask, legend. Together with .FileFunction it defines
/// the role of that image in the layer structure of the PCB. Note that the .FilePolarity
/// attribute does not change the image - no attribute does. It changes the interpretation of
/// the image. For example, in a copper layer in positive polarity a round flash generates a
/// copper pad. In a copper layer in negative polarity it generates a clearance. Solder mask
/// images usually represent solder mask openings and are then negative. This may be
/// counter-intuitive.
pub fn gerber_file_polarity_attribute(a_layer: i32) -> String {
    let polarity = match PcbLayerId(a_layer) {
        PcbLayerId::FAdhes
        | PcbLayerId::BAdhes
        | PcbLayerId::FSilkS
        | PcbLayerId::BSilkS
        | PcbLayerId::FPaste
        | PcbLayerId::BPaste => 1,

        PcbLayerId::FMask | PcbLayerId::BMask => -1,

        _ => {
            if is_copper_layer(a_layer) {
                1
            } else {
                0
            }
        }
    };

    match polarity {
        1 => "%TF.FilePolarity,Positive*%".to_string(),
        -1 => "%TF.FilePolarity,Negative*%".to_string(),
        _ => String::new(),
    }
}

/// Port of `makeStringCompatX1` (`static`): convert a X2 attribute string to a X1 structured
/// comment (remove every `%`, prepend `G04 #@! `) when `a_use_x1_compatibility_mode`.
fn make_string_compat_x1(a_text: &mut String, a_use_x1_compatibility_mode: bool) {
    if a_use_x1_compatibility_mode {
        *a_text = a_text.replace('%', "");
        a_text.insert_str(0, "G04 #@! ");
    }
}

/// Port of `replaceReservedCharsField` (`static`): replace reserved chars (separators in
/// gerber fields) in a gerber string field: `,` by `_`, `*` and `%` by an escaped sequence.
fn replace_reserved_chars_field(a_msg: &mut String) {
    *a_msg = a_msg.replace(',', "_"); // can be replaced by \\u002C
    *a_msg = a_msg.replace('*', "\\u002A");
    *a_msg = a_msg.replace('%', "\\u0025");
}

/// `wxString::ToAscii()` (default replacement `_`): every code point above 0x7F becomes `_`.
fn wx_to_ascii(a_text: &str) -> String {
    a_text
        .chars()
        .map(|c| if (c as u32) < 0x80 { c } else { '_' })
        .collect()
}

/// `ExpandTextVars( aSource, aBoard->GetProject(), FOR_GUI )`.
///
/// TEMPORARY: the board's `PROJECT::TextVarResolver` is private in `kicad-pcb`
/// (`board/text_vars.rs`, `project_text_var_resolver`); this mirrors it (same token list,
/// empty clock, empty VCS hash) until the model exposes it (change request of FAB-03), and
/// must then be replaced by that call. Without a project the C++ would dereference a null
/// project; kicad-cli always binds one, the port resolves nothing.
fn expand_project_text_vars(a_board: &Board, a_source: &str) -> String {
    let Some(project) = &a_board.project else {
        return expand_text_vars(a_source, None, ResolutionContext::ForGui);
    };

    let resolver = |token: &mut String| -> bool {
        match token.as_str() {
            "PROJECTNAME" => {
                *token = WxFileName::new(&a_board.file_name).name().to_string();
                true
            }
            "CURRENT_DATE" | "CURRENT_TIME_HH_MM_SS" | "CURRENT_TIME_LOCALE" | "VCSHASH"
            | "VCSSHORTHASH" => {
                *token = String::new();
                true
            }
            _ => match project.text_vars.get(token.as_str()) {
                Some(value) => {
                    *token = value.clone();
                    true
                }
                None => false,
            },
        }
    };

    expand_text_vars(a_source, Some(&resolver), ResolutionContext::ForGui)
}

/// Port of `AddGerberX2Header`: add the X2 file header attributes `TF.GenerationSoftware`,
/// `TF.CreationDate`, `TF.ProjectId` and `TF.SameCoordinates` (as defined in the Gerber file
/// format specification J4 and "Revision 2015.06") to the plotter header, as `G04 #@!`
/// structured comments in X1 compatibility mode.
pub fn add_gerber_x2_header(
    a_plotter: &mut dyn Plotter,
    a_board: &Board,
    a_env: &PlotEnv,
    a_use_x1_compatibility_mode: bool,
) {
    // Creates the TF,.GenerationSoftware. Format is:
    // %TF,.GenerationSoftware,<vendor>,<application name>[,<application version>]*%
    let mut text = format!(
        "%TF.GenerationSoftware,KiCad,Pcbnew,{}*%",
        a_env.build_version
    );
    make_string_compat_x1(&mut text, a_use_x1_compatibility_mode);
    a_plotter.add_line_to_header(&text);

    // creates the TF.CreationDate attribute:
    let text = gbr_make_creation_date_attribute_string(
        if a_use_x1_compatibility_mode {
            GbrNcStringFormat::X1
        } else {
            GbrNcStringFormat::X2
        },
        &a_env.creation_date,
    );
    a_plotter.add_line_to_header(&text);

    // Creates the TF,.ProjectId. Format is (from Gerber file format doc):
    // %TF.ProjectId,<project id>,<project GUID>,<revision id>*%
    // <project id> is the name of the project, restricted to basic ASCII symbols only,
    // Rem: <project id> accepts only ASCII 7 code (only basic ASCII codes are allowed in
    // gerber files) and comma not accepted.
    // All illegal chars will be replaced by underscore.
    //
    // <project GUID> is a string which is an unique id of a project.
    // However Kicad does not handle such a project GUID, so it is built from the board name
    let fn_ = WxFileName::new(&a_board.file_name);
    let msg = fn_.full_name();

    // Build a <project GUID>, from the board name
    let guid = gbr_make_project_guid_from_string(&msg);

    // build the <project id> string: this is the board short filename (without ext)
    // and all non ASCII chars and reserved chars (, * % ) are replaced by '_'
    let mut msg = fn_.name().to_string();
    replace_reserved_chars_field(&mut msg);

    // build the <revision id> string. All non ASCII chars and reserved chars are replaced by '_'
    let mut rev = expand_project_text_vars(a_board, a_board.titles.revision());
    replace_reserved_chars_field(&mut rev);

    if rev.is_empty() {
        rev = "rev?".to_string();
    }

    let mut text = format!(
        "%TF.ProjectId,{},{},{}*%",
        wx_to_ascii(&msg),
        guid,
        wx_to_ascii(&rev)
    );
    make_string_compat_x1(&mut text, a_use_x1_compatibility_mode);
    a_plotter.add_line_to_header(&text);

    // Add the TF.SameCoordinates to specify that all gerber files uses the same origin and
    // orientation, and the registration between files is OK.
    // The parameter of TF.SameCoordinates is a string that is common to all files using the
    // same registration.  The string value has no meaning; it is just a key.
    // Because there is no mirroring/rotation in Kicad, only the plot offset origin can create
    // incorrect registration, so we create a key from plot offset options.
    //
    // Currently the key is "Original" when using absolute Pcbnew coordinates, and the PY and PY
    // position of the auxiliary axis when using it.
    // If we ever add user-settable absolute Pcbnew coordinates, we'll need to change the way
    // the key is built to ensure file only using the *same* axis have the same key.
    let mut registration_id = "Original".to_string();
    let aux_origin = a_board.design_settings().aux_origin();

    // Note: the board's stored plot options, not the options of the current plot.
    if a_board.plot_options.use_aux_origin && aux_origin.x != 0 && aux_origin.y != 0 {
        // wxString::Printf( "%x" ) of an int: the 32-bit two's complement for negative values
        registration_id = format!("PX{:x}PY{:x}", aux_origin.x as u32, aux_origin.y as u32);
    }

    let mut text = format!("%TF.SameCoordinates,{registration_id}*%");
    make_string_compat_x1(&mut text, a_use_x1_compatibility_mode);
    a_plotter.add_line_to_header(&text);
}

/// Port of `AddGerberX2Attribute`: the X2 header of [`add_gerber_x2_header`], then
/// `TF.FileFunction` and, for layers which support it, `TF.FilePolarity`.
pub fn add_gerber_x2_attribute(
    a_plotter: &mut dyn Plotter,
    a_board: &Board,
    a_layer: i32,
    a_env: &PlotEnv,
    a_use_x1_compatibility_mode: bool,
) {
    add_gerber_x2_header(a_plotter, a_board, a_env, a_use_x1_compatibility_mode);

    // Add the TF.FileFunction
    let mut text = gerber_file_function_attribute(a_board, a_layer);
    make_string_compat_x1(&mut text, a_use_x1_compatibility_mode);
    a_plotter.add_line_to_header(&text);

    // Add the TF.FilePolarity (for layers which support that)
    let mut text = gerber_file_polarity_attribute(a_layer);

    if !text.is_empty() {
        make_string_compat_x1(&mut text, a_use_x1_compatibility_mode);
        a_plotter.add_line_to_header(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protel_extensions() {
        let cases = [
            (PcbLayerId::FCu, "gtl"),
            (PcbLayerId::BCu, "gbl"),
            (PcbLayerId::In1Cu, "g1"),
            (PcbLayerId::In2Cu, "g2"),
            (PcbLayerId::In30Cu, "g30"),
            (PcbLayerId::BAdhes, "gba"),
            (PcbLayerId::FAdhes, "gta"),
            (PcbLayerId::BPaste, "gbp"),
            (PcbLayerId::FPaste, "gtp"),
            (PcbLayerId::BSilkS, "gbo"),
            (PcbLayerId::FSilkS, "gto"),
            (PcbLayerId::BMask, "gbs"),
            (PcbLayerId::FMask, "gts"),
            (PcbLayerId::EdgeCuts, "gm1"),
            (PcbLayerId::DwgsUser, "gbr"),
            (PcbLayerId::Margin, "gbr"),
            (PcbLayerId::FFab, "gbr"),
        ];

        for (layer, ext) in cases {
            assert_eq!(gerber_protel_extension(layer.0), ext, "{layer:?}");
        }
    }

    #[test]
    fn polarity() {
        assert_eq!(
            gerber_file_polarity_attribute(PcbLayerId::In5Cu.0),
            "%TF.FilePolarity,Positive*%"
        );
        assert_eq!(
            gerber_file_polarity_attribute(PcbLayerId::BMask.0),
            "%TF.FilePolarity,Negative*%"
        );
        assert_eq!(gerber_file_polarity_attribute(PcbLayerId::EdgeCuts.0), "");
        assert_eq!(gerber_file_polarity_attribute(PcbLayerId::FFab.0), "");
    }

    #[test]
    fn x1_compat_and_reserved_chars() {
        let mut t = "%TF.FileFunction,Copper,L1,Top*%".to_string();
        make_string_compat_x1(&mut t, true);
        assert_eq!(t, "G04 #@! TF.FileFunction,Copper,L1,Top*");

        let mut m = "a,b*c%d".to_string();
        replace_reserved_chars_field(&mut m);
        assert_eq!(m, "a_b\\u002Ac\\u0025d");
        assert_eq!(wx_to_ascii("x\u{e9}\u{1F600}y"), "x__y");
    }
}
