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

//! Port of `pcbnew/exporters/gerber_jobfile_writer.h` / `.cpp` (`.gbrjob`).
//!
//! Work package FAB-09 (`docs/analysis/plot-fab.md` 4.16, 6.7). The JSON document is an
//! `nlohmann::ordered_json` in C++; here a `serde_json` [`Value`] with insertion-ordered objects,
//! dumped with the nlohmann-compatible writer `kicad_core::json::dump_stream` (`std::setw( 2 )
//! << m_json << std::endl`).
//!
//! `GetBuildVersion()` and the clock read by `GbrMakeCreationDateAttributeString` are
//! caller-supplied ([`PlotEnv`]). The file is produced in memory ([`OutputFile`]).

use kicad_core::PcbLayerId;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::color4d::Color4D;
use kicad_core::json::{Map, ObjectOrder, Value, dump_stream, float_value};
use kicad_core::layer_ids::{is_copper_layer, is_user_layer};
use kicad_core::printf::format_f;
use kicad_core::report_severity::Severity;
use kicad_core::text_vars::{ResolutionContext, expand_text_vars};
use kicad_pcb::board::Board;
use kicad_pcb::board_stackup::{
    BoardStackupItemType, BsEdgeConnectorConstraints, is_prm_specified, standard_colors,
};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::padstack::PadProp;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::traits::{BoardConnectedItem, BoardItem};
use kicad_plot::gbr_metadata::{
    GbrNcStringFormat, gbr_make_creation_date_attribute_string, gbr_make_project_guid_from_string,
};

use crate::pcbplot::PlotEnv;
use crate::wx_file_name::WxFileName;
use crate::{OutputFile, Reporter};

/// `ONSIDE`: a helper enum to handle sides of some layers (silk, mask).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum OnSide {
    /// `SIDE_NONE`: layers not present.
    None = 0,
    /// `SIDE_TOP`: top layer only.
    Top = 1,
    /// `SIDE_BOTTOM`: bottom layer only.
    Bottom = 2,
    /// `SIDE_BOTH`: both layers.
    Both = 3,
}

impl OnSide {
    /// `(enum ONSIDE) flag`.
    fn from_flag(flag: i32) -> OnSide {
        match flag {
            1 => OnSide::Top,
            2 => OnSide::Bottom,
            3 => OnSide::Both,
            _ => OnSide::None,
        }
    }
}

/// `JOBFILE_PARAMS`: the list of parameters written in a Gerber job file, especially the list
/// of `.gbr` file names and the corresponding layer ids belonging to the job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobfileParams {
    /// `m_GerberFileList`: the Gerber file names (without path).
    pub gerber_file_list: Vec<String>,
    /// `m_LayerId`: the corresponding layer ids.
    pub layer_id: Vec<PcbLayerId>,
}

/// `GERBER_JOBFILE_WRITER`: creates a Gerber job file, which stores the info to make a board:
/// the list of Gerber files and info about the board itself (size, number of copper layers,
/// thickness of the board, copper and dielectric, and some other info such as colours and
/// finish type).
///
/// Note: dimensions are always in mm in a KiCad job file (they can also be in inches in a job
/// file) and they are in floating point notation.
pub struct GerberJobfileWriter<'a, 'r> {
    /// `m_pcb` (with the DRC engine and connectivity of the board, for `GetOwnClearance`).
    pcb: BoardCtx<'a>,
    /// `m_reporter`: a reporter for messages (can be null).
    reporter: Option<&'r mut dyn Reporter>,
    /// `m_params`: the list of various parameters and data to write in a job file.
    params: JobfileParams,
    /// `m_conversionUnits`: scaling factor to convert board units to Gerber units (mm).
    conversion_units: f64,
    /// `m_json`: the JSON document built by this class.
    json: Value,
}

/// `m_json[aKey]` on an object: the member, created as `null` when missing (nlohmann
/// `operator[]` on an object; a `null` value is first turned into an object).
fn member<'v>(value: &'v mut Value, key: &str) -> &'v mut Value {
    if value.is_null() {
        *value = Value::Object(Map::new());
    }

    match value {
        Value::Object(map) => map.entry(key.to_string()).or_insert(Value::Null),
        // nlohmann throws type_error.305 here; the writer never indexes a non-object.
        _ => unreachable!("operator[] with a string key on a non-object"),
    }
}

/// `m_json[aIndex]` on an array (every index used by the writer exists).
fn element(value: &mut Value, index: usize) -> &mut Value {
    match value {
        Value::Array(items) => &mut items[index],
        _ => unreachable!("operator[] with an index on a non-array"),
    }
}

/// `m_json[aKey] = aValue` on an object.
fn set(value: &mut Value, key: &str, new_value: Value) {
    *member(value, key) = new_value;
}

/// `push_back` / `operator+=` / `insert( end(), ... )` on an array (a `null` value first
/// becomes an array, as in nlohmann).
fn push(value: &mut Value, new_value: Value) {
    if value.is_null() {
        *value = Value::Array(Vec::new());
    }

    if let Value::Array(items) = value {
        items.push(new_value);
    }
}

/// A JSON string value.
fn string(text: &str) -> Value {
    Value::String(text.to_string())
}

/// `ExpandTextVars( aSource, aBoard->GetProject(), FOR_GUI )`.
///
/// TEMPORARY: a copy of the private `pcbplot::expand_project_text_vars` (itself a mirror of the
/// board's private `PROJECT::TextVarResolver`, same token list, empty clock, empty VCS hash)
/// until the model exposes the project resolver; then both must call it.
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
            "CURRENT_DATE"
            | "CURRENT_TIME_HH_MM_SS"
            | "CURRENT_TIME_LOCALE"
            | "VCSHASH"
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

/// Port of `BOARD::GetPadWithPressFitAttrCount`: the number of pads with the Press-Fit
/// fabrication property.
///
/// TEMPORARY: not declared by the board model yet (change request of FAB-09).
fn pad_with_press_fit_attr_count(a_board: &Board) -> i32 {
    // return the number of PTH with Press-Fit fabr attribute
    let mut count = 0;

    for (fp, _) in a_board.footprints() {
        for (_, pad) in a_board.footprint_pads(fp) {
            if pad.property() == PadProp::Pressfit {
                count += 1;
            }
        }
    }

    count
}

/// Port of `BOARD::GetPadWithCastellatedAttrCount`: the number of pads with the Castellated
/// fabrication property.
///
/// TEMPORARY: not declared by the board model yet (change request of FAB-09).
fn pad_with_castellated_attr_count(a_board: &Board) -> i32 {
    // @return the number of PTH with Castellated fabr attribute
    let mut count = 0;

    for (fp, _) in a_board.footprints() {
        for (_, pad) in a_board.footprint_pads(fp) {
            if pad.property() == PadProp::Castellated {
                count += 1;
            }
        }
    }

    count
}

/// Port of `GERBER_JOBFILE_WRITER::mapValue` without the conversion factor: `"%.4f"` of `a_mm`
/// parsed back into a number.
///
/// The C++ parses the text with `sscanf( "%Lg" )` into a `long double` and converts that to
/// `double`. That double rounding equals the direct parse for every `N / 10^4` with
/// `0 <= N < 10^8` (exhaustive check, `docs/THIRD_PARTY_EQUIVALENTS.md` 3.4); beyond that
/// (values above 10 m) the two can differ in the last bit. There is no `long double` on wasm32.
pub fn map_value_mm(a_mm: f64) -> f64 {
    let buffer = format_f(a_mm, 4);
    buffer.parse::<f64>().unwrap_or(0.0)
}

impl<'a, 'r> GerberJobfileWriter<'a, 'r> {
    /// Port of `GERBER_JOBFILE_WRITER( aPcb, aReporter )`.
    pub fn new(a_pcb: BoardCtx<'a>, a_reporter: Option<&'r mut dyn Reporter>) -> Self {
        GerberJobfileWriter {
            pcb: a_pcb,
            reporter: a_reporter,
            params: JobfileParams::default(),
            conversion_units: 1.0 / PCB_IU_SCALE.iu_per_mm, // Gerber units = mm
            json: Value::Null,
        }
    }

    /// Port of `AddGbrFile`: add a Gerber file name and type to the job file list.
    ///
    /// * `a_layer`: the layer of the Gerber file.
    /// * `a_filename`: the file name (without path) of the Gerber file.
    pub fn add_gbr_file(&mut self, a_layer: PcbLayerId, a_filename: &str) {
        self.params.gerber_file_list.push(a_filename.to_string());
        self.params.layer_id.push(a_layer);
    }

    /// The parameters collected by [`Self::add_gbr_file`].
    pub fn params(&self) -> &JobfileParams {
        &self.params
    }

    /// Port of `formatStringFromUTF32`: the UTF-8 form of a text (a Rust string already is).
    fn format_string_from_utf32(a_text: &str) -> String {
        a_text.to_string()
    }

    /// Port of `hasSilkLayers`: which silk screen layers are in the list.
    pub fn has_silk_layers(&self) -> OnSide {
        let mut flag = OnSide::None as i32;

        for &layer in &self.params.layer_id {
            if layer == PcbLayerId::BSilkS {
                flag |= OnSide::Bottom as i32;
            }

            if layer == PcbLayerId::FSilkS {
                flag |= OnSide::Top as i32;
            }
        }

        OnSide::from_flag(flag)
    }

    /// Port of `hasSolderMasks`: which solder mask layers are in the list.
    pub fn has_solder_masks(&self) -> OnSide {
        let mut flag = OnSide::None as i32;

        for &layer in &self.params.layer_id {
            if layer == PcbLayerId::BMask {
                flag |= OnSide::Bottom as i32;
            }

            if layer == PcbLayerId::FMask {
                flag |= OnSide::Top as i32;
            }
        }

        OnSide::from_flag(flag)
    }

    /// Port of `sideKeyValue`: the key associated to the sides used for some layers.
    pub fn side_key_value(a_value: OnSide) -> &'static str {
        // return the key associated to sides used for some layers
        // "No, TopOnly, BotOnly or Both"
        match a_value {
            OnSide::None => "No",
            OnSide::Top => "TopOnly",
            OnSide::Bottom => "BotOnly",
            OnSide::Both => "Both",
        }
    }

    /// Port of `CreateJobFile( aFullFilename )`: the Gerber job file `a_full_filename`.
    ///
    /// The C++ result is always `true` (`WriteJSONJobFile` never fails: a stream that cannot
    /// be opened is not checked), so the port returns the file itself.
    pub fn create_job_file(&mut self, a_full_filename: &str, a_env: &PlotEnv) -> OutputFile {
        let file = self.write_json_job_file(a_full_filename, a_env);

        if let Some(reporter) = self.reporter.as_deref_mut() {
            let msg = format!("Created Gerber job file '{a_full_filename}'.");
            reporter.report(&msg, Severity::Action);
        }

        file
    }

    /// Port of `WriteJSONJobFile( aFullFilename )`: the Gerber job file in JSON format.
    pub fn write_json_job_file(&mut self, a_full_filename: &str, a_env: &PlotEnv) -> OutputFile {
        // Note: in Gerber job file, dimensions are in mm, and are floating numbers
        self.json = Value::Object(Map::new());

        // output the job file header
        self.add_json_header(a_env);

        // Add the General Specs
        self.add_json_general_specs();

        // Job file support a few design rules:
        self.add_json_design_rules();

        // output the gerber file list:
        self.add_json_files_attributes();

        // output the board stackup:
        self.add_json_material_stackup();

        let text = dump_stream(&self.json, 2, ObjectOrder::Insertion);

        OutputFile {
            name: a_full_filename.to_string(),
            contents: text.into_bytes(),
        }
    }

    /// Port of `mapValue`: convert `a_ui_value` (board units) to JSON units (mm) with 4 digits
    /// in the mantissa when printed with `%g`, i.e. values truncated to 0.1 micron. This is
    /// enough for a JSON file.
    fn map_value(&self, a_ui_value: f64) -> f64 {
        map_value_mm(a_ui_value * self.conversion_units)
    }

    /// Port of `addJSONHeader`.
    fn add_json_header(&mut self, a_env: &PlotEnv) {
        let mut generation_software = Map::new();
        generation_software.insert("Vendor".to_string(), string("KiCad"));
        generation_software.insert("Application".to_string(), string("Pcbnew"));
        generation_software.insert("Version".to_string(), string(&a_env.build_version));

        let mut header = Map::new();
        header.insert(
            "GenerationSoftware".to_string(),
            Value::Object(generation_software),
        );
        // The attribute value must conform to the full version of the ISO 8601
        // date and time format, including time and time zone.
        header.insert(
            "CreationDate".to_string(),
            string(&gbr_make_creation_date_attribute_string(
                GbrNcStringFormat::Gbrjob,
                &a_env.creation_date,
            )),
        );

        set(&mut self.json, "Header", Value::Object(header));
    }

    /// Port of `addJSONGeneralSpecs`.
    fn add_json_general_specs(&mut self) {
        let brd = self.pcb.board;
        let ctx = self.pcb;
        let mut specs = Value::Object(Map::new());
        set(&mut specs, "ProjectId", Value::Object(Map::new()));

        // Creates the ProjectId. Format is (from Gerber file format doc):
        // ProjectId,<project id>,<project GUID>,<revision id>*%
        // <project id> is the name of the project, restricted to basic ASCII symbols only,
        // and comma not accepted
        // All illegal chars will be replaced by underscore
        // Rem: <project id> accepts only ASCII 7 code (only basic ASCII codes are allowed in
        // gerber files).
        //
        // <project GUID> is a string which is an unique id of a project.
        // However Kicad does not handle such a project GUID, so it is built from the board name
        let fn_ = WxFileName::new(&brd.file_name);
        let msg = fn_.full_name();

        // Build a <project GUID>, from the board name
        let guid = gbr_make_project_guid_from_string(&msg);

        // build the <project id> string: this is the board short filename (without ext)
        // and in UTF8 format.
        let msg = fn_.name().to_string();

        // build the <rev> string. All non ASCII chars are in UTF8 form
        let mut rev = expand_project_text_vars(brd, brd.titles.revision());

        if rev.is_empty() {
            rev = "rev?".to_string();
        }

        {
            let project_id = member(&mut specs, "ProjectId");
            set(project_id, "Name", string(&msg));
            set(project_id, "GUID", string(&guid));
            set(project_id, "Revision", string(&rev));
        }

        // output the board size in mm:
        let brect = brd.board_edges_bounding_box(&ctx);

        {
            let size = member(&mut specs, "Size");
            set(size, "X", float_value(self.map_value(brect.width() as f64)));
            set(
                size,
                "Y",
                float_value(self.map_value(brect.height() as f64)),
            );
        }

        // Add some data to the JSON header, GeneralSpecs:
        // number of copper layers
        set(
            &mut specs,
            "LayerNumber",
            Value::from(brd.copper_layer_count()),
        );

        // Board thickness
        set(
            &mut specs,
            "BoardThickness",
            float_value(self.map_value(brd.design_settings().board_thickness() as f64)),
        );

        // Copper finish
        let brd_stackup = brd.design_settings().stackup_descriptor();

        if !brd_stackup.finish_type.is_empty() {
            set(&mut specs, "Finish", string(&brd_stackup.finish_type));
        }

        if brd_stackup.has_dielectric_constrains {
            set(&mut specs, "ImpedanceControlled", Value::Bool(true));
        }

        // (`#if 0` in C++: the old way to set the property from m_CastellatedPads.)
        if pad_with_castellated_attr_count(brd) != 0 {
            set(&mut specs, "Castellated", Value::Bool(true));
        }

        if pad_with_press_fit_attr_count(brd) != 0 {
            set(&mut specs, "Press-fit", Value::Bool(true));
        }

        if brd_stackup.edge_plating {
            set(&mut specs, "EdgePlating", Value::Bool(true));
        }

        if brd_stackup.edge_connector_constraints != BsEdgeConnectorConstraints::None {
            set(&mut specs, "EdgeConnector", Value::Bool(true));

            set(
                &mut specs,
                "EdgeConnectorBevelled",
                Value::Bool(
                    brd_stackup.edge_connector_constraints == BsEdgeConnectorConstraints::Bevelled,
                ),
            );
        }

        // (`#if 0` in C++, not yet in use: "IPC-2221-Type" and "ViaProtection".)

        set(&mut self.json, "GeneralSpecs", specs);
    }

    /// Port of `addJSONFilesAttributes`.
    fn add_json_files_attributes(&mut self) {
        // Add the Files Attributes section in JSON format to m_JSONbuffer
        let mut files = Value::Array(Vec::new());
        let copper_layer_count = self.pcb.board.copper_layer_count();

        for ii in 0..self.params.gerber_file_list.len() {
            let name = &self.params.gerber_file_list[ii];
            let layer = self.params.layer_id[ii];
            let mut gbr_layer_id = String::new();
            let mut skip_file = false; // true to skip files which should not be in job file
            let mut polarity = "Positive";

            if is_copper_layer(layer) {
                gbr_layer_id.push_str("Copper,L");

                if layer == PcbLayerId::BCu {
                    gbr_layer_id.push_str(&copper_layer_count.to_string());
                } else if layer == PcbLayerId::FCu {
                    gbr_layer_id.push('1');
                } else {
                    // Copper layers are numbered B_Cu + n*2 for inner layer n (n = 1 ... val max)
                    // and gbr_layer_id = 2 ... val max
                    gbr_layer_id.push_str(&((layer.0 - PcbLayerId::BCu.0) / 2 + 1).to_string());
                }

                gbr_layer_id.push(',');

                if layer == PcbLayerId::BCu {
                    gbr_layer_id.push_str("Bot");
                } else if layer == PcbLayerId::FCu {
                    gbr_layer_id.push_str("Top");
                } else {
                    gbr_layer_id.push_str("Inr");
                }
            } else if is_user_layer(layer) {
                gbr_layer_id = "Other,User".to_string();
            } else {
                match layer {
                    PcbLayerId::BAdhes => gbr_layer_id = "Glue,Bot".to_string(),
                    PcbLayerId::FAdhes => gbr_layer_id = "Glue,Top".to_string(),

                    PcbLayerId::BPaste => gbr_layer_id = "SolderPaste,Bot".to_string(),
                    PcbLayerId::FPaste => gbr_layer_id = "SolderPaste,Top".to_string(),

                    PcbLayerId::BSilkS => gbr_layer_id = "Legend,Bot".to_string(),
                    PcbLayerId::FSilkS => gbr_layer_id = "Legend,Top".to_string(),

                    PcbLayerId::BMask => {
                        gbr_layer_id = "SolderMask,Bot".to_string();
                        polarity = "Negative";
                    }
                    PcbLayerId::FMask => {
                        gbr_layer_id = "SolderMask,Top".to_string();
                        polarity = "Negative";
                    }

                    PcbLayerId::EdgeCuts => gbr_layer_id = "Profile".to_string(),

                    PcbLayerId::BFab => gbr_layer_id = "AssemblyDrawing,Bot".to_string(),
                    PcbLayerId::FFab => gbr_layer_id = "AssemblyDrawing,Top".to_string(),

                    PcbLayerId::Margin | PcbLayerId::BCrtYd | PcbLayerId::FCrtYd => {
                        skip_file = true;
                    }

                    _ => {
                        skip_file = true;

                        if let Some(reporter) = self.reporter.as_deref_mut() {
                            reporter.report("Unexpected layer id in job file", Severity::Error);
                        }
                    }
                }
            }

            if !skip_file {
                // name can contain non ASCII7 chars.
                // Ensure the name is JSON compatible.
                let strname = Self::format_string_from_utf32(name);
                let mut file_json = Value::Null;

                set(&mut file_json, "Path", string(&strname));
                set(&mut file_json, "FileFunction", string(&gbr_layer_id));
                set(&mut file_json, "FilePolarity", string(polarity));

                push(&mut files, file_json);
            }
        }

        set(&mut self.json, "FilesAttributes", files);
    }

    /// Port of `addJSONDesignRules`.
    fn add_json_design_rules(&mut self) {
        // Add the Design Rules section in JSON format to m_JSONbuffer
        // Job file support a few design rules:
        let ctx = self.pcb;
        let brd = ctx.board;
        let net_settings = &brd.design_settings().net_settings;

        let mut minclearance_outer = net_settings.default_netclass().clearance();
        let has_inner_layers = brd.copper_layer_count() > 2;

        // Search a smaller clearance in other net classes, if any.
        for netclass in net_settings.netclasses().values() {
            if netclass.has_clearance() {
                minclearance_outer = minclearance_outer.min(netclass.clearance());
            }
        }

        // job file knows different clearance types.
        // Kicad knows only one clearance for pads and tracks
        let minclearance_track2track = minclearance_outer;

        // However, pads can have a specific clearance defined for a pad or a footprint,
        // and min clearance can be dependent on layers.
        // Search for a minimal pad clearance:
        let mut min_pad_clearance_outer = net_settings.default_netclass().clearance();
        let mut min_pad_clearance_inner = net_settings.default_netclass().clearance();

        for (fp, _) in brd.footprints() {
            for (_, pad) in brd.footprint_pads(fp) {
                for layer in pad.layer_set(&ctx).iter() {
                    let pad_clearance = pad.own_clearance(&ctx, layer, false).0;

                    if layer == PcbLayerId::BCu || layer == PcbLayerId::FCu {
                        min_pad_clearance_outer = min_pad_clearance_outer.min(pad_clearance);
                    } else {
                        min_pad_clearance_inner = min_pad_clearance_inner.min(pad_clearance);
                    }
                }
            }
        }

        let mut outer = Map::new();
        outer.insert("Layers".to_string(), string("Outer"));
        outer.insert(
            "PadToPad".to_string(),
            float_value(self.map_value(min_pad_clearance_outer as f64)),
        );
        outer.insert(
            "PadToTrack".to_string(),
            float_value(self.map_value(min_pad_clearance_outer as f64)),
        );
        outer.insert(
            "TrackToTrack".to_string(),
            float_value(self.map_value(minclearance_track2track as f64)),
        );
        let mut rules = Value::Array(vec![Value::Object(outer)]);

        // Until this is changed in Kicad, use the same value for internal tracks
        // (int minclearanceInner = minclearanceOuter; overwritten below before any use)

        // Output the minimal track width
        let mut mintrack_width_outer = i32::MAX;
        let mut mintrack_width_inner = i32::MAX;

        for (_, track) in brd.tracks() {
            if matches!(track, Track::Via(_)) {
                continue;
            }

            let layer = track.layer(&ctx);

            if layer == PcbLayerId::BCu || layer == PcbLayerId::FCu {
                mintrack_width_outer = mintrack_width_outer.min(track.width());
            } else {
                mintrack_width_inner = mintrack_width_inner.min(track.width());
            }
        }

        if mintrack_width_outer != i32::MAX {
            set(
                element(&mut rules, 0),
                "MinLineWidth",
                float_value(self.map_value(mintrack_width_outer as f64)),
            );
        }

        // Output the minimal zone to xx clearance
        // Note: zones can have a zone clearance set to 0
        // if happens, the actual zone clearance is the clearance of its class
        let mut minclearance_outer = i32::MAX;
        let mut minclearance_inner = i32::MAX;

        for (_, zone) in brd.zones() {
            if zone.is_rule_area() || !zone.is_on_copper_layer(&ctx) {
                continue;
            }

            for layer in zone.layer_set(&ctx).iter() {
                let zclerance = BoardConnectedItem::own_clearance(zone, &ctx, layer, false).0;

                if layer == PcbLayerId::BCu || layer == PcbLayerId::FCu {
                    minclearance_outer = minclearance_outer.min(zclerance);
                } else {
                    minclearance_inner = minclearance_inner.min(zclerance);
                }
            }
        }

        if minclearance_outer != i32::MAX {
            set(
                element(&mut rules, 0),
                "TrackToRegion",
                float_value(self.map_value(minclearance_outer as f64)),
            );
        }

        if minclearance_outer != i32::MAX {
            set(
                element(&mut rules, 0),
                "RegionToRegion",
                float_value(self.map_value(minclearance_outer as f64)),
            );
        }

        if has_inner_layers {
            let mut inner = Map::new();
            inner.insert("Layers".to_string(), string("Inner"));
            inner.insert(
                "PadToPad".to_string(),
                float_value(self.map_value(min_pad_clearance_inner as f64)),
            );
            inner.insert(
                "PadToTrack".to_string(),
                float_value(self.map_value(min_pad_clearance_inner as f64)),
            );
            inner.insert(
                "TrackToTrack".to_string(),
                float_value(self.map_value(minclearance_track2track as f64)),
            );
            push(&mut rules, Value::Object(inner));

            if mintrack_width_inner != i32::MAX {
                set(
                    element(&mut rules, 1),
                    "MinLineWidth",
                    float_value(self.map_value(mintrack_width_inner as f64)),
                );
            }

            if minclearance_inner != i32::MAX {
                set(
                    element(&mut rules, 1),
                    "TrackToRegion",
                    float_value(self.map_value(minclearance_inner as f64)),
                );
            }

            if minclearance_inner != i32::MAX {
                set(
                    element(&mut rules, 1),
                    "RegionToRegion",
                    float_value(self.map_value(minclearance_inner as f64)),
                );
            }
        }

        set(&mut self.json, "DesignRules", rules);
    }

    /// Port of `addJSONMaterialStackup`: the ordered list of stackup layers (mask, paste,
    /// silk, copper, dielectric) used to make the physical board. Therefore not all layers are
    /// listed here.
    fn add_json_material_stackup(&mut self) {
        // Add the Material Stackup section in JSON format to m_JSONbuffer
        let mut material_stackup = Value::Array(Vec::new());
        let brd = self.pcb.board;

        // Build the candidates list:
        let mut brd_stackup = brd.design_settings().stackup_descriptor().clone();

        // Ensure brd_stackup is up to date (i.e. no change made by SynchronizeWithBoard() )
        let uptodate = !brd_stackup.synchronize_with_board(brd.design_settings());

        if !uptodate && brd.design_settings().has_stackup {
            if let Some(reporter) = self.reporter.as_deref_mut() {
                reporter.report("Board stackup settings not up to date.", Severity::Error);
            }
        }

        let mut last_copper_layer = PcbLayerId::FCu;

        // Generate the list (top to bottom):
        for ii in 0..brd_stackup.count() {
            let Some(item) = brd_stackup.stackup_layer(ii) else {
                continue;
            };

            let sub_layer_count = if item.type_() == BoardStackupItemType::Dielectric {
                item.sublayers_count()
            } else {
                1
            };

            for sub_idx in 0..sub_layer_count {
                // layer thickness is always in mm
                let thickness = self.map_value(item.thickness(sub_idx) as f64);
                let mut layer_type = String::new();
                let mut layer_name = String::new(); // for comment

                let mut layer_json = Value::Null;

                match item.type_() {
                    BoardStackupItemType::Copper => {
                        layer_type = "Copper".to_string();
                        layer_name =
                            Self::format_string_from_utf32(&brd.layer_name(item.brd_layer_id()));
                        last_copper_layer = item.brd_layer_id();
                    }

                    BoardStackupItemType::Silkscreen => {
                        layer_type = "Legend".to_string();
                        layer_name = Self::format_string_from_utf32(item.type_name());
                    }

                    BoardStackupItemType::Soldermask => {
                        layer_type = "SolderMask".to_string();
                        layer_name = Self::format_string_from_utf32(item.type_name());
                    }

                    BoardStackupItemType::Solderpaste => {
                        layer_type = "SolderPaste".to_string();
                        layer_name = Self::format_string_from_utf32(item.type_name());
                    }

                    BoardStackupItemType::Dielectric => {
                        layer_type = "Dielectric".to_string();
                        // The option core or prepreg is not added here, as it creates
                        // constraints in build process, not necessary wanted.
                        if sub_layer_count > 1 {
                            layer_name = Self::format_string_from_utf32(&format!(
                                "dielectric layer {} - {}/{}",
                                item.dielectric_layer_id(),
                                sub_idx + 1,
                                sub_layer_count
                            ));
                        } else {
                            layer_name = Self::format_string_from_utf32(&format!(
                                "dielectric layer {}",
                                item.dielectric_layer_id()
                            ));
                        }
                    }

                    BoardStackupItemType::Undefined => {}
                }

                set(&mut layer_json, "Type", string(&layer_type));

                if item.is_color_editable() && uptodate && is_prm_specified(&item.color(sub_idx)) {
                    let mut color_name = item.color(sub_idx);

                    if color_name.starts_with('#') {
                        // This is a user defined color, not in standard color list.
                        // In job file a color can be given by its RGB values (0...255)
                        // like R<number><G<number>B<number> notation
                        let color = Color4D::from_string(&color_name).to_colour();
                        color_name = format!("R{}G{}B{}", color.red, color.green, color.blue);
                    } else {
                        let color_list = standard_colors(item.type_());

                        // Colors for dielectric use a color list that is mainly not normalized
                        // in job file names. So if a color is in the dielectric standard color
                        // list it can be a standard name or not.
                        // Colors for solder mask and silk screen use a mainly normalized
                        // color list, but this list can also contain not normalized colors.
                        // If not normalized, use the R<number><G<number>B<number> notation
                        for prm_color in color_list {
                            if color_name == prm_color.name() {
                                color_name = prm_color.color_as_string();
                                break;
                            }
                        }
                    }

                    set(&mut layer_json, "Color", string(&color_name));
                }

                if item.is_thickness_editable() && uptodate {
                    set(&mut layer_json, "Thickness", float_value(thickness));
                }

                if item.type_() == BoardStackupItemType::Dielectric {
                    if item.has_material_value(0) {
                        set(&mut layer_json, "Material", string(&item.material(sub_idx)));

                        // These constrains are only written if the board has impedance
                        // controlled tracks. If the board is not impedance controlled, they are
                        // useless. Do not add constrains that create more expensive boards.

                        if brd_stackup.has_dielectric_constrains {
                            // Generate Epsilon R if > 1.0 (value <= 1.0 means not specified:
                            // it is not a possible value
                            if item.epsilon_r(0) > 1.0 {
                                set(
                                    &mut layer_json,
                                    "DielectricConstant",
                                    string(&item.format_epsilon_r(sub_idx)),
                                );
                            }

                            // Generate LossTangent > 0.0 (value <= 0.0 means not specified: it
                            // is not a possible value
                            if item.loss_tangent(0) > 0.0 {
                                set(
                                    &mut layer_json,
                                    "LossTangent",
                                    string(&item.format_loss_tangent(sub_idx)),
                                );
                            }
                        }
                    }

                    // Copper layers IDs use only even values like 0, 2, 4 ...
                    // and first layer = F_Cu = 0, last layer = B_Cu = 2
                    // inner layers Ids are 4, 6 , 8 ...
                    let mut next_copper_layer = PcbLayerId(last_copper_layer.0 + 2);

                    if last_copper_layer == PcbLayerId::FCu {
                        next_copper_layer = PcbLayerId::In1Cu;
                    }

                    // If the next_copper_layer is the last copper layer, the next layer id is
                    // B_Cu
                    if next_copper_layer.0 / 2 >= brd.copper_layer_count() {
                        next_copper_layer = PcbLayerId::BCu;
                    }

                    let mut sub_layer_name = String::new();

                    if sub_layer_count > 1 {
                        sub_layer_name = format!(" ({}/{})", sub_idx + 1, sub_layer_count);
                    }

                    let last_name =
                        Self::format_string_from_utf32(&brd.layer_name(last_copper_layer));
                    let next_name =
                        Self::format_string_from_utf32(&brd.layer_name(next_copper_layer));

                    let name = format!("{last_name}/{next_name}{sub_layer_name}");

                    set(&mut layer_json, "Name", string(&name));

                    // Add a comment ("Notes"):
                    let mut note = String::new();

                    note.push_str(&format!("Type: {layer_name}"));

                    note.push_str(&format!(" (from {last_name} to {next_name})"));

                    set(&mut layer_json, "Notes", string(&note));
                } else if item.type_() == BoardStackupItemType::Soldermask
                    || item.type_() == BoardStackupItemType::Silkscreen
                {
                    if item.has_material_value(0) {
                        set(&mut layer_json, "Material", string(&item.material(0)));

                        // These constrains are only written if the board has impedance
                        // controlled tracks. If the board is not impedance controlled, they are
                        // useless. Do not add constrains that create more expensive boards.
                        if brd_stackup.has_dielectric_constrains {
                            // Generate Epsilon R if > 1.0 (value <= 1.0 means not specified:
                            // it is not a possible value
                            if item.epsilon_r(0) > 1.0 {
                                set(
                                    &mut layer_json,
                                    "DielectricConstant",
                                    string(&item.format_epsilon_r(0)),
                                );
                            }

                            // Generate LossTangent > 0.0 (value <= 0.0 means not specified: it
                            // is not a possible value
                            if item.loss_tangent(0) > 0.0 {
                                set(
                                    &mut layer_json,
                                    "LossTangent",
                                    string(&item.format_loss_tangent(0)),
                                );
                            }
                        }
                    }

                    set(&mut layer_json, "Name", string(&layer_name));
                } else {
                    set(&mut layer_json, "Name", string(&layer_name));
                }

                push(&mut material_stackup, layer_json);
            }
        }

        set(&mut self.json, "MaterialStackup", material_stackup);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_value_rounds_to_four_decimals() {
        assert_eq!(map_value_mm(1.6), 1.6);
        assert_eq!(map_value_mm(0.035), 0.035);
        assert_eq!(map_value_mm(0.00004999), 0.0);
        assert_eq!(map_value_mm(0.00005001), 0.0001);
        assert_eq!(map_value_mm(123.456789), 123.4568);
        assert_eq!(map_value_mm(-0.2), -0.2);
        // glibc rounds the exact binary value: 0.00015 is below the decimal midpoint.
        assert_eq!(map_value_mm(0.00015), 0.0001);
        assert_eq!(map_value_mm(1e-6 * 2_147_483_647.0), 2147.4836);
    }

    #[test]
    fn side_keys() {
        assert_eq!(GerberJobfileWriter::side_key_value(OnSide::None), "No");
        assert_eq!(GerberJobfileWriter::side_key_value(OnSide::Top), "TopOnly");
        assert_eq!(
            GerberJobfileWriter::side_key_value(OnSide::Bottom),
            "BotOnly"
        );
        assert_eq!(GerberJobfileWriter::side_key_value(OnSide::Both), "Both");
    }
}
