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

//! Port of `pcbnew/exporters/gendrill_writer_base.h` and of `gendrill_writer_base.cpp`
//! L47-300 (`cmpHoleSorting`, `buildHolesList`, `layerName`, `layerPairName`,
//! `getDrillFileName`, `getProtectionFileName`), L303-355 (`CreateMapFilesSet`), L358-461
//! (`BuildFileFunctionAttributeString`) and L464-501 (unit / pen size helpers).
//!
//! Not here: `genDrillMapFile` and `plotDrillMarks` (drill maps, FAB-12, `gendrill_map.rs`),
//! `GenDrillReportFile` and `printToolSummary` (drill report, FAB-02).
//!
//! The C++ class is an abstract base whose derived writers (`EXCELLON_WRITER`,
//! `GERBER_WRITER`) embed it here as a field; its protected members are `pub` so that they
//! can. `m_pcb` is the board context the writer is created with. Files are not written:
//! [`GendrillWriterBase::outputs`] collects them for the host.

use kicad_core::layer_ids::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_core::plot_enums::PlotFormat;
use kicad_core::report_severity::Severity;
use kicad_core::{PCB_IU_SCALE, PageInfo};
use kicad_geom::Vector2I;
use kicad_geom::stdsort::std_sort;
use kicad_pcb::BoardCtx;
use kicad_pcb::padstack::PadDrillPostMachiningMode;
use kicad_plot::common_plot_functions::get_default_plot_extension;

use crate::drill::{DrillLayerPair, DrillOperation, DrillQuery, DrillSpan, HoleAttribute};
use crate::drill::{enumerate_drill_operations, enumerate_drill_spans};
use crate::wx_file_name::WxFileName;
use crate::{FabError, OutputFile, Reporter};

/// `USE_ATTRIB_FOR_HOLES`: holes can have an attribute in Excellon drill files, similar to
/// attributes in Gerber X2 format. They are only comments for a better identification of
/// holes (vias, pads...). The C++ build sets it to 1.
pub const USE_ATTRIB_FOR_HOLES: bool = true;

/// `IPC4761_FEATURES`: via protection features according to IPC-4761.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum Ipc4761Features {
    Filled,
    Capped,
    PluggedFront,
    PluggedBack,
    CoveredFront,
    CoveredBack,
    TentedFront,
    TentedBack,
}

/// `DRILL_TOOL`: a tool used in the drill file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillTool {
    /// The diameter of the used tool (for oblong, the smaller size).
    pub diameter: i32,
    /// How many times it is used (round and oblong).
    pub total_count: i32,
    /// Oblong count.
    pub oval_count: i32,
    /// Is the hole plated or not plated.
    pub hole_not_plated: bool,
    /// Attribute (used in Excellon drill file).
    pub hole_attribute: HoleAttribute,
    /// True when drilling a backdrill span.
    pub is_backdrill: bool,
    /// True if any hole for this tool has post-machining.
    pub has_post_machining: bool,
    /// Minimum stub length for this tool (IU).
    pub min_stub_length: Option<i32>,
    /// Maximum stub length for this tool (IU).
    pub max_stub_length: Option<i32>,
}

impl DrillTool {
    /// `DRILL_TOOL( aDiameter, a_NotPlated )`.
    pub fn new(diameter: i32, not_plated: bool) -> Self {
        DrillTool {
            diameter,
            total_count: 0,
            oval_count: 0,
            hole_not_plated: not_plated,
            hole_attribute: HoleAttribute::HoleUnknown,
            is_backdrill: false,
            has_post_machining: false,
            min_stub_length: None,
            max_stub_length: None,
        }
    }
}

/// `DRILL_PRECISION`: drill precision format in Excellon files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillPrecision {
    /// Left digit number (integer value of coordinates).
    pub lhs: i32,
    /// Right digit number (decimal value of coordinates).
    pub rhs: i32,
}

impl Default for DrillPrecision {
    /// `DRILL_PRECISION( l = 2, r = 4 )`.
    fn default() -> Self {
        DrillPrecision::new(2, 4)
    }
}

impl DrillPrecision {
    /// `DRILL_PRECISION( l, r )`.
    pub const fn new(l: i32, r: i32) -> Self {
        DrillPrecision { lhs: l, rhs: r }
    }

    /// `DRILL_PRECISION::GetPrecisionString()`: `"<lhs>:<rhs>"`.
    pub fn precision_string(&self) -> String {
        format!("{}:{}", self.lhs, self.rhs)
    }
}

/// `GENDRILL_WRITER_BASE::ZEROS_FMT`: zero format in coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum ZerosFmt {
    /// Floating point coordinates.
    #[default]
    DecimalFormat,
    /// Suppress leading zeros.
    SuppressLeading,
    /// Suppress trailing zeros.
    SuppressTrailing,
    /// Keep zeros.
    KeepZeros,
}

/// `GENDRILL_WRITER_BASE::TYPE_FILE`: type of holes in file: PTH, NPTH, mixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum TypeFile {
    /// PTH only, this is the default also for blind/buried holes.
    PthFile,
    /// NPTH only.
    NpthFile,
    /// PHT+NPTH (mixed).
    MixedFile,
}

/// `GENDRILL_WRITER_BASE::TOOL_SUMMARY`: selects which subset of the tool list a summary
/// covers (drill report, FAB-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolSummary {
    /// Plated holes.
    Plated,
    /// Non-plated holes, excluding backdrills.
    Unplated,
    /// Backdrills, which are non-plated by construction.
    Backdrill,
}

/// Port of the static `cmpHoleSorting`: helper function for sorting the hole list:
/// plated then not plated, then by increasing diameter value, then by attribute type (vias,
/// pad, mechanical), then by X then Y position. Distinct holes can compare equal (same
/// plating, diameter, attribute and position), so it is used with `std_sort`.
pub fn cmp_hole_sorting(a: &DrillOperation, b: &DrillOperation) -> bool {
    if a.not_plated != b.not_plated {
        return b.not_plated;
    }

    if a.diameter != b.diameter {
        return a.diameter < b.diameter;
    }

    // At this point (same diameter, same plated type), group by attribute
    // type (via, pad, mechanical, although currently only not plated pads are mechanical)
    if a.attribute != b.attribute {
        return a.attribute < b.attribute;
    }

    // At this point (same diameter, same type), sort by X then Y position.
    // This is optimal for drilling and make the file reproducible as long as holes
    // have not changed, even if the data order has changed.
    if a.position.x != b.position.x {
        return a.position.x < b.position.x;
    }

    a.position.y < b.position.y
}

/// Port of the inline `diameter_in_inches( double ius )`.
pub fn diameter_in_inches(ius: f64) -> f64 {
    ius * 0.001 / PCB_IU_SCALE.iu_per_mils
}

/// Port of the inline `diameter_in_mm( double ius )`.
pub fn diameter_in_mm(ius: f64) -> f64 {
    ius / PCB_IU_SCALE.iu_per_mm
}

/// Port of the static `getMarkerBestPenSize`: a pen size to plot markers and having a
/// readable shape, clamped to be >= `MIN_SIZE_MM` to avoid too small line width.
pub fn get_marker_best_pen_size(marker_diameter: i32) -> i32 {
    let mut bestsize = marker_diameter / 10;

    const MIN_SIZE_MM: f64 = 0.1;
    bestsize = bestsize.max(PCB_IU_SCALE.mm_to_iu(MIN_SIZE_MM));

    bestsize
}

/// Port of the inline `getSketchOvalBestPenSize`: a pen size to plot outlines for oval holes.
pub fn get_sketch_oval_best_pen_size() -> i32 {
    const SKETCH_LINE_WIDTH_MM: f64 = 0.1;
    PCB_IU_SCALE.mm_to_iu(SKETCH_LINE_WIDTH_MM)
}

/// Port of the inline `getDefaultPenSize`: a default pen size to plot items with no specific
/// line thickness.
pub fn get_default_pen_size() -> i32 {
    const DEFAULT_LINE_WIDTH_MM: f64 = 0.2;
    PCB_IU_SCALE.mm_to_iu(DEFAULT_LINE_WIDTH_MM)
}

/// Port of `GENDRILL_WRITER_BASE::layerName`: `front`, `back` or `in<n>`.
pub fn layer_name(layer: PcbLayerId) -> String {
    // Generic names here.
    match layer {
        PcbLayerId::FCu => "front".to_string(),
        PcbLayerId::BCu => "back".to_string(),
        _ => {
            // aLayer use even values, and the first internal layer (In1) is B_Cu + 2.
            let ly_id = (layer.0 - PcbLayerId::BCu.0) / 2;
            format!("in{ly_id}")
        }
    }
}

/// Port of `GENDRILL_WRITER_BASE::layerPairName`: `"<layer1Name>-<layer2Name>"`, used to
/// generate a filename for drill files and drill maps.
pub fn layer_pair_name(pair: DrillLayerPair) -> String {
    let mut ret = layer_name(pair.0);
    ret.push('-');
    ret.push_str(&layer_name(pair.1));

    ret
}

/// `GENDRILL_WRITER_BASE`: create drill maps and drill reports and drill files. Drill files
/// are created by the specialized writers, depending on the file format.
#[derive(Clone)]
pub struct GendrillWriterBase<'a> {
    /// `m_pcb`.
    pub pcb: BoardCtx<'a>,
    /// `.drl` or `.gbr`, depending on format (`m_drillFileExtension`).
    pub drill_file_extension: String,
    /// true = mm, false = inches (`m_unitsMetric`).
    pub units_metric: bool,
    /// The zero format option for output file (`m_zeroFormat`).
    pub zero_format: ZerosFmt,
    /// The current coordinate precision, not used in decimal format (`m_precision`).
    pub precision: DrillPrecision,
    /// Scaling factor to convert the board units to Excellon/Gerber units, i.e. inches or mm
    /// (`m_conversionUnits`).
    pub conversion_units: f64,
    /// Drill offset coordinates (`m_offset`).
    pub offset: Vector2I,
    /// True to generate only one drill file (`m_merge_PTH_NPTH`).
    pub merge_pth_npth: bool,
    /// `m_holeListBuffer`: the sorted hole list of the last `build_holes_list`.
    pub hole_list_buffer: Vec<DrillOperation>,
    /// One-based tool numbers, indexed by the sorted operation list (`m_holeToolReferences`).
    pub hole_tool_references: Vec<i32>,
    /// Buffer containing tools (`m_toolListBuffer`).
    pub tool_list_buffer: Vec<DrillTool>,
    /// The format of the map drill file, if this map is needed (`m_mapFileFmt`).
    pub map_file_fmt: PlotFormat,
    /// The page info used to plot drill maps; `None` = A4 (`m_pageInfo`).
    pub page_info: Option<PageInfo>,
    /// `m_createdFiles`.
    pub created_files: Vec<String>,
    /// The files written so far (the port's replacement for the file system).
    pub outputs: Vec<OutputFile>,
}

impl<'a> GendrillWriterBase<'a> {
    /// `GENDRILL_WRITER_BASE( aPcb )` (protected: used by the derived writers).
    pub fn new(pcb: BoardCtx<'a>) -> Self {
        GendrillWriterBase {
            pcb,
            drill_file_extension: String::new(),
            units_metric: true,
            zero_format: ZerosFmt::DecimalFormat,
            precision: DrillPrecision::default(),
            conversion_units: 1.0,
            offset: Vector2I::new(0, 0),
            merge_pth_npth: false,
            hole_list_buffer: Vec::new(),
            hole_tool_references: Vec::new(),
            tool_list_buffer: Vec::new(),
            map_file_fmt: PlotFormat::Pdf,
            page_info: None,
            created_files: Vec::new(),
            outputs: Vec::new(),
        }
    }

    /// `SetMergeOption`: true to make only one file containing PTH and NPTH, false to create
    /// 2 separate files.
    pub fn set_merge_option(&mut self, merge: bool) {
        self.merge_pth_npth = merge;
    }

    /// `GetOffset`: the plot offset (usually the position of the drill/place origin).
    pub fn offset(&self) -> Vector2I {
        self.offset
    }

    /// `SetPageInfo`: the page info used to plot drill maps (`None`: A4).
    pub fn set_page_info(&mut self, page_info: Option<PageInfo>) {
        self.page_info = page_info;
    }

    /// `SetMapFileFormat`.
    pub fn set_map_file_format(&mut self, map_fmt: PlotFormat) {
        self.map_file_fmt = map_fmt;
    }

    /// `GetDrillFileExt`: the file extension of the drill writer format.
    pub fn drill_file_ext(&self) -> &str {
        &self.drill_file_extension
    }

    /// `GetCreatedFiles`.
    pub fn created_files(&self) -> &[String] {
        &self.created_files
    }

    /// `AddCreatedFile`.
    pub fn add_created_file(&mut self, path: &str) {
        self.created_files.push(path.to_string());
    }

    /// `getHolesCount`.
    pub fn holes_count(&self) -> usize {
        self.hole_list_buffer.len()
    }

    /// Port of `GENDRILL_WRITER_BASE::buildHolesList( aSpan, aGenerateNPTH_list )`: create
    /// the list of holes and tools for the span, sorted by increasing drill size. If the span
    /// is `[F_Cu, B_Cu]`, pad holes are included too. `generate_npth_list`: true for the NPTH
    /// only list (no plated holes), false for the plated list (no NPTH unless merged).
    pub fn build_holes_list(&mut self, span: &DrillSpan, generate_npth_list: bool) {
        self.hole_list_buffer.clear();
        self.tool_list_buffer.clear();
        self.hole_tool_references.clear();

        let query = DrillQuery {
            span: *span,
            non_plated_only: generate_npth_list,
            merge_pth_npth: self.merge_pth_npth,
            ..DrillQuery::default()
        };

        let holes = enumerate_drill_operations(&self.pcb, &query);
        self.build_tool_list(holes);
    }

    /// The second half of `buildHolesList` (`gendrill_writer_base.cpp` L90-162), split off so
    /// that a hand-built enumeration can be fed in: sort the holes with `cmpHoleSorting`
    /// (`std::sort`, libstdc++ tie order) and build the tool list and the per-hole tool
    /// references.
    pub fn build_tool_list(&mut self, holes: Vec<DrillOperation>) {
        self.hole_list_buffer = holes;
        self.tool_list_buffer.clear();
        self.hole_tool_references.clear();

        std_sort(&mut self.hole_list_buffer, cmp_hole_sorting);

        self.hole_tool_references
            .resize(self.hole_list_buffer.len(), -1);

        let mut last_hole: i32 = -1;
        let mut last_notplated_opt = false;
        let mut last_attribute = HoleAttribute::HoleUnknown;

        let mut new_tool = DrillTool::new(0, false);

        for ii in 0..self.hole_list_buffer.len() {
            let hole = &self.hole_list_buffer[ii];

            if hole.diameter != last_hole
                || hole.not_plated != last_notplated_opt
                || (USE_ATTRIB_FOR_HOLES && hole.attribute != last_attribute)
            {
                new_tool.diameter = hole.diameter;
                new_tool.hole_not_plated = hole.not_plated;
                new_tool.hole_attribute = hole.attribute;
                self.tool_list_buffer.push(new_tool.clone());
                last_hole = new_tool.diameter;
                last_notplated_opt = new_tool.hole_not_plated;
                last_attribute = new_tool.hole_attribute;
            }

            let jj = self.tool_list_buffer.len();

            if jj == 0 {
                continue; // Should not occurs
            }

            self.hole_tool_references[ii] = jj as i32;

            let tool = self.tool_list_buffer.last_mut().expect("jj > 0");

            tool.total_count += 1;

            if hole.is_slot {
                tool.oval_count += 1;
            }

            if hole.is_backdrill() {
                tool.is_backdrill = true;

                if let Some(stub) = hole.stub_length {
                    if tool.min_stub_length.is_none_or(|min| stub < min) {
                        tool.min_stub_length = Some(stub);
                    }

                    if tool.max_stub_length.is_none_or(|max| stub > max) {
                        tool.max_stub_length = Some(stub);
                    }
                }
            }

            if hole.front_post_machining.mode == PadDrillPostMachiningMode::Counterbore
                || hole.front_post_machining.mode == PadDrillPostMachiningMode::Countersink
                || hole.back_post_machining.mode == PadDrillPostMachiningMode::Counterbore
                || hole.back_post_machining.mode == PadDrillPostMachiningMode::Countersink
                || hole.is_backdrill()
            {
                tool.has_post_machining = true;
            }
        }
    }

    /// Port of `GENDRILL_WRITER_BASE::getDrillFileName( aSpan, aNPTH, aMerge_PTH_NPTH )`
    /// (the base-class version; `GERBER_WRITER` overrides it): the board file name (no
    /// directory) with `-PTH`, `-NPTH`, `-<pair>` or `_Backdrills_Drill_<a>_<b>` added and the
    /// writer's extension.
    pub fn drill_file_name(&self, span: &DrillSpan, npth: bool, merge_pth_npth: bool) -> String {
        let board = self.pcb.board;

        let mut extend = String::new();

        let layer_index = |layer: PcbLayerId| -> i32 {
            let mut conventional_layer_num = 1;

            for l in Lset::all_cu_mask_n(board.copper_layer_count()).ui_order().0 {
                if l == layer {
                    return conventional_layer_num;
                }

                conventional_layer_num += 1;
            }

            conventional_layer_num
        };

        if span.is_backdrill {
            extend = format!(
                "_Backdrills_Drill_{}_{}",
                layer_index(span.drill_start_layer()),
                layer_index(span.drill_end_layer())
            );
        } else if npth {
            extend = "-NPTH".to_string();
        } else if span.pair() == (PcbLayerId::FCu, PcbLayerId::BCu) {
            if !merge_pth_npth {
                extend = "-PTH".to_string();
            }
            // if merged, extend with nothing
        } else {
            extend.push('-');
            extend.push_str(&layer_pair_name(span.pair()));
        }

        let mut fn_ = WxFileName::new(&board.file_name);

        fn_.set_name(&format!("{}{}", fn_.name(), extend));
        fn_.set_ext(&self.drill_file_extension);

        fn_.full_name()
    }

    /// Port of `GENDRILL_WRITER_BASE::getProtectionFileName( aSpan, aFeature )`: the board
    /// file name followed by the feature name and the layer(s) associated with it.
    pub fn protection_file_name(&self, span: &DrillSpan, feature: Ipc4761Features) -> String {
        let board = self.pcb.board;

        let mut extend = String::new();

        match feature {
            Ipc4761Features::Filled => {
                extend.push_str("-filling-");
                extend.push_str(&layer_pair_name(span.pair()));
            }
            Ipc4761Features::Capped => {
                extend.push_str("-capping-");
                extend.push_str(&layer_pair_name(span.pair()));
            }
            Ipc4761Features::CoveredBack => {
                extend.push_str("-covering-");
                extend.push_str(&layer_name(span.pair().1));
            }
            Ipc4761Features::CoveredFront => {
                extend.push_str("-covering-");
                extend.push_str(&layer_name(span.pair().0));
            }
            Ipc4761Features::PluggedBack => {
                extend.push_str("-plugging-");
                extend.push_str(&layer_name(span.pair().1));
            }
            Ipc4761Features::PluggedFront => {
                extend.push_str("-plugging-");
                extend.push_str(&layer_name(span.pair().0));
            }
            Ipc4761Features::TentedBack => {
                extend.push_str("-tenting-");
                extend.push_str(&layer_name(span.pair().1));
            }
            Ipc4761Features::TentedFront => {
                extend.push_str("-tenting-");
                extend.push_str(&layer_name(span.pair().0));
            }
        }

        let mut fn_ = WxFileName::new(&board.file_name);

        fn_.set_name(&format!("{}{}", fn_.name(), extend));
        fn_.set_ext(&self.drill_file_extension);

        fn_.full_name()
    }

    /// Port of `GENDRILL_WRITER_BASE::CreateMapFilesSet( aPlotDirectory, aReporter )`: one
    /// drill map per drill file (`<drill file name without extension>-drl_map.<ext>`), in the
    /// format of `map_file_fmt`.
    ///
    /// `env` gives the build version and creation date of the maps (see
    /// [`super::gendrill_map::gen_drill_map_file`]). A map format without a plot engine in
    /// the port (PDF, PostScript, DXF) fails with `FabError::Unsupported` after the C++
    /// failure message is reported. C++ returns `false` on failure; the port returns the
    /// error that caused it.
    pub fn create_map_files_set(
        &mut self,
        plot_directory: &str,
        env: &crate::pcbplot::PlotEnv,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), FabError> {
        let mut hole_sets = enumerate_drill_spans(&self.pcb, false);

        if !self.merge_pth_npth {
            hole_sets.push(DrillSpan::new(
                PcbLayerId::FCu,
                PcbLayerId::BCu,
                false,
                true,
            ));
        }

        for span in &hole_sets {
            let doing_npth = span.is_non_plated_file;

            self.build_holes_list(span, doing_npth);

            if self.holes_count() > 0
                || doing_npth
                || span.pair() == (PcbLayerId::FCu, PcbLayerId::BCu)
            {
                let mut fn_ =
                    WxFileName::new(&self.drill_file_name(span, doing_npth, self.merge_pth_npth));
                fn_.set_path(plot_directory);

                fn_.set_ext(""); // Will be added by GenDrillMap
                let fullfilename = format!(
                    "{}-drl_map.{}",
                    fn_.full_path(),
                    get_default_plot_extension(self.map_file_fmt)
                );

                match super::gendrill_map::gen_drill_map_file(
                    self,
                    &fullfilename,
                    self.map_file_fmt,
                    env,
                ) {
                    Err(err) => {
                        if let Some(rep) = reporter.as_deref_mut() {
                            let msg = format!("Failed to create file '{fullfilename}'.");
                            rep.report(&msg, Severity::Error);
                        }

                        return Err(err);
                    }
                    Ok(contents) => {
                        if let Some(rep) = reporter.as_deref_mut() {
                            let msg = format!("Created file '{fullfilename}'.");
                            rep.report(&msg, Severity::Action);
                        }

                        self.outputs.push(OutputFile {
                            name: fullfilename.clone(),
                            contents,
                        });
                        self.add_created_file(&fullfilename);
                    }
                }
            }
        }

        Ok(())
    }

    /// Port of `GENDRILL_WRITER_BASE::BuildFileFunctionAttributeString( aSpan, aHoleType,
    /// aCompatNCdrill )`: the standard X2 FileFunction for drill files,
    /// `%TF.FileFunction,Plated[NonPlated],layer1num,layer2num,PTH[NPTH][Blind][Buried],
    /// Drill[Route][Mixed]*%`. There is no X1 version, as the Gerber drill files use only X2;
    /// with `compat_nc_drill` the NC (Excellon) comment form `; #@! TF.FileFunction,...`
    /// without the last field and the closing `*%` is returned. The last field depends on the
    /// current hole list.
    pub fn build_file_function_attribute_string(
        &self,
        span: &DrillSpan,
        hole_type: TypeFile,
        compat_nc_drill: bool,
    ) -> String {
        // Build a wxString containing the .FileFunction attribute for drill files.
        // %TF.FileFunction,Plated[NonPlated],layer1num,layer2num,PTH[NPTH][Blind][Buried],Drill[Route][Mixed]*%
        let copper_layer_count = self.pcb.board.copper_layer_count();

        let mut text = if compat_nc_drill {
            "; #@! ".to_string()
        } else {
            "%".to_string()
        };

        text.push_str("TF.FileFunction,");

        if span.is_backdrill || hole_type == TypeFile::NpthFile {
            text.push_str("NonPlated,");
        } else if hole_type == TypeFile::MixedFile {
            // only for Excellon format
            text.push_str("MixedPlating,");
        } else {
            text.push_str("Plated,");
        }

        let mut layer1 = span.pair().0.0;
        let mut layer2 = span.pair().1.0;

        // In Gerber files, layers num are 1 to copper layer count instead of F_Cu to B_Cu
        // (0 to copper layer count-1)
        // Note also for a n copper layers board, gerber layers num are 1 ... n
        //
        // Copper layers use even values, so the layer id in file is
        // (Copper layer id) /2 + 1 if layer is not B_Cu
        let f_cu = PcbLayerId::FCu.0;
        let b_cu = PcbLayerId::BCu.0;

        if layer1 == f_cu {
            layer1 = 1;
        } else if layer1 == b_cu {
            layer1 = copper_layer_count;
        } else {
            layer1 = ((layer1 - b_cu) / 2) + 1;
        }

        if layer2 == f_cu {
            layer2 = 1;
        } else if layer2 == b_cu {
            layer2 = copper_layer_count;
        } else {
            layer2 = ((layer2 - b_cu) / 2) + 1;
        }

        // Ensure layer order is from top (smaller layer number) to bottom (bigger layer number)
        if layer1 > layer2 {
            std::mem::swap(&mut layer1, &mut layer2);
        }

        text.push_str(&format!("{layer1},{layer2}"));

        // Now add PTH or NPTH or Blind or Buried attribute
        let toplayer = 1;
        let bottomlayer = copper_layer_count;

        if span.is_backdrill {
            text.push_str(",Blind");
        } else if hole_type == TypeFile::NpthFile {
            text.push_str(",NPTH");
        } else if hole_type == TypeFile::MixedFile {
            // only for Excellon format: write nothing
        } else if layer1 == toplayer && layer2 == bottomlayer {
            text.push_str(",PTH");
        } else if layer1 == toplayer || layer2 == bottomlayer {
            text.push_str(",Blind");
        } else {
            text.push_str(",Buried");
        }

        // In NC drill file, these previous parameters should be enough:
        if compat_nc_drill {
            return text;
        }

        // Now add Drill or Route or Mixed:
        // file containing only round holes have Drill attribute
        // file containing only oblong holes have Routed attribute
        // file containing both holes have Mixed attribute
        let mut has_oblong = false;
        let mut has_drill = false;

        for hole_descr in &self.hole_list_buffer {
            if hole_descr.is_slot {
                has_oblong = true;
            } else {
                has_drill = true;
            }
        }

        if has_oblong && has_drill {
            text.push_str(",Mixed");
        } else if has_drill {
            text.push_str(",Drill");
        } else if has_oblong {
            text.push_str(",Rout");
        }

        // else: empty file.

        // End of .FileFunction attribute:
        text.push_str("*%");

        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drill::DrillOpKind;
    use kicad_pcb::Board;

    const MM: i32 = 1_000_000;

    fn board(file: &str, copper: i32) -> Board {
        let mut b = Board::default();
        b.file_name = file.to_string();
        b.set_copper_layer_count(copper);
        b
    }

    fn hole(dia: i32, not_plated: bool, attr: HoleAttribute, x: i32, y: i32) -> DrillOperation {
        DrillOperation {
            diameter: dia,
            size_xy: Vector2I::new(dia, dia),
            not_plated,
            attribute: attr,
            position: Vector2I::new(x, y),
            ..DrillOperation::default()
        }
    }

    #[test]
    fn holes_sorted_plated_first_then_diameter_attribute_x_y() {
        let b = board("/p/demo.kicad_pcb", 2);
        let mut w = GendrillWriterBase::new(BoardCtx::bare(&b));
        use HoleAttribute::*;
        w.build_tool_list(vec![
            hole(MM, true, HoleMechanical, 0, 0),
            hole(MM, false, HolePad, 5, 0),
            hole(MM, false, HoleViaThrough, 9, 9),
            hole(MM / 2, false, HolePad, 0, 0),
            hole(MM, false, HolePad, 5, -1),
            hole(MM, false, HolePad, 1, 7),
        ]);
        let got: Vec<_> = w
            .hole_list_buffer
            .iter()
            .map(|h| {
                (
                    h.not_plated,
                    h.diameter,
                    h.attribute,
                    h.position.x,
                    h.position.y,
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (false, MM / 2, HolePad, 0, 0),
                (false, MM, HoleViaThrough, 9, 9),
                (false, MM, HolePad, 1, 7),
                (false, MM, HolePad, 5, -1),
                (false, MM, HolePad, 5, 0),
                (true, MM, HoleMechanical, 0, 0),
            ]
        );
        // A new tool whenever diameter, plating or attribute changes.
        assert_eq!(w.hole_tool_references, vec![1, 2, 3, 3, 3, 4]);
        let tools: Vec<_> = w
            .tool_list_buffer
            .iter()
            .map(|t| {
                (
                    t.diameter,
                    t.hole_not_plated,
                    t.hole_attribute,
                    t.total_count,
                )
            })
            .collect();
        assert_eq!(
            tools,
            vec![
                (MM / 2, false, HolePad, 1),
                (MM, false, HoleViaThrough, 1),
                (MM, false, HolePad, 3),
                (MM, true, HoleMechanical, 1),
            ]
        );
    }

    #[test]
    fn tool_counts_slots_stubs_and_post_machining() {
        let b = board("demo.kicad_pcb", 4);
        let mut w = GendrillWriterBase::new(BoardCtx::bare(&b));
        let mut slot = hole(MM, false, HoleAttribute::HolePad, 0, 0);
        slot.is_slot = true;
        let mut pm = hole(MM, false, HoleAttribute::HolePad, 1, 0);
        pm.front_post_machining.mode = PadDrillPostMachiningMode::Countersink;
        let mut not_pm = hole(MM, false, HoleAttribute::HolePad, 2, 0);
        not_pm.back_post_machining.mode = PadDrillPostMachiningMode::NotPostMachined;
        let bd = |x, stub| {
            let mut h = hole(2 * MM, true, HoleAttribute::HoleViaBackdrill, x, 0);
            h.kind = DrillOpKind::SecondaryDrill;
            h.stub_length = stub;
            h
        };
        w.build_tool_list(vec![
            slot,
            pm,
            not_pm,
            bd(0, Some(300)),
            bd(1, None),
            bd(2, Some(100)),
            bd(3, Some(200)),
        ]);
        assert_eq!(w.tool_list_buffer.len(), 2);
        let t = &w.tool_list_buffer[0];
        assert_eq!(
            (
                t.total_count,
                t.oval_count,
                t.has_post_machining,
                t.is_backdrill
            ),
            (3, 1, true, false)
        );
        let t = &w.tool_list_buffer[1];
        assert_eq!(
            (t.total_count, t.is_backdrill, t.has_post_machining),
            (4, true, true)
        );
        assert_eq!(
            (t.min_stub_length, t.max_stub_length),
            (Some(100), Some(300))
        );

        // Only "not post machined": no flag.
        w.build_tool_list(vec![hole(MM, false, HoleAttribute::HolePad, 2, 0), {
            let mut h = hole(MM, false, HoleAttribute::HolePad, 3, 0);
            h.back_post_machining.mode = PadDrillPostMachiningMode::NotPostMachined;
            h
        }]);
        assert!(!w.tool_list_buffer[0].has_post_machining);
        assert_eq!(w.tool_list_buffer[0].min_stub_length, None);
    }

    #[test]
    fn empty_list_and_unset_diameter() {
        let b = board("demo.kicad_pcb", 2);
        let mut w = GendrillWriterBase::new(BoardCtx::bare(&b));
        w.build_tool_list(Vec::new());
        assert!(w.tool_list_buffer.is_empty() && w.hole_tool_references.is_empty());
        // A first hole equal to the initial "last" values (-1, plated, unknown) gets no tool
        // and keeps the reference -1, as in C++.
        w.build_tool_list(vec![hole(-1, false, HoleAttribute::HoleUnknown, 0, 0)]);
        assert!(w.tool_list_buffer.is_empty());
        assert_eq!(w.hole_tool_references, vec![-1]);
    }

    #[test]
    fn drill_file_names() {
        let b = board("/home/u/proj/demo.kicad_pcb", 4);
        let mut w = GendrillWriterBase::new(BoardCtx::bare(&b));
        w.drill_file_extension = "drl".to_string();
        let (f, b_, in1, in2) = (
            PcbLayerId::FCu,
            PcbLayerId::BCu,
            PcbLayerId::In1Cu,
            PcbLayerId::In2Cu,
        );
        let through = DrillSpan::new(f, b_, false, false);
        assert_eq!(w.drill_file_name(&through, false, true), "demo.drl");
        assert_eq!(w.drill_file_name(&through, false, false), "demo-PTH.drl");
        assert_eq!(w.drill_file_name(&through, true, false), "demo-NPTH.drl");
        assert_eq!(w.drill_file_name(&through, true, true), "demo-NPTH.drl");
        assert_eq!(
            w.drill_file_name(&DrillSpan::new(f, in1, false, false), false, true),
            "demo-front-in1.drl"
        );
        assert_eq!(
            w.drill_file_name(&DrillSpan::new(in1, in2, false, false), false, false),
            "demo-in1-in2.drl"
        );
        // DrillFileLayerOrderInFilename (qa/tests/pcbnew/test_cam_backdrill.cpp), naming part:
        // a via from In2 to B_Cu gives "in2-back", not "back-in2".
        assert_eq!(
            w.drill_file_name(&DrillSpan::new(b_, in2, false, false), false, false),
            "demo-in2-back.drl"
        );
        // Backdrills: 1-based positions of the stored start / end layer in UIOrder
        // (F_Cu = 1, In1 = 2, In2 = 3, B_Cu = 4), stored order kept.
        assert_eq!(
            w.drill_file_name(&DrillSpan::new(b_, in2, true, false), false, false),
            "demo_Backdrills_Drill_4_3.drl"
        );
        assert_eq!(
            w.drill_file_name(&DrillSpan::new(f, in1, true, false), true, true),
            "demo_Backdrills_Drill_1_2.drl"
        );
        // A layer outside the board's copper set gets count + 1.
        assert_eq!(
            w.drill_file_name(
                &DrillSpan::new(f, PcbLayerId::In5Cu, true, false),
                false,
                false
            ),
            "demo_Backdrills_Drill_1_5.drl"
        );
        w.drill_file_extension = "gbr".to_string();
        assert_eq!(w.drill_file_name(&through, false, false), "demo-PTH.gbr");
    }

    #[test]
    fn protection_file_names() {
        let b = board("demo.kicad_pcb", 6);
        let mut w = GendrillWriterBase::new(BoardCtx::bare(&b));
        w.drill_file_extension = "gbr".to_string();
        let span = DrillSpan::new(PcbLayerId::BCu, PcbLayerId::In2Cu, false, false);
        use Ipc4761Features::*;
        let got: Vec<_> = [
            Filled,
            Capped,
            PluggedFront,
            PluggedBack,
            CoveredFront,
            CoveredBack,
            TentedFront,
            TentedBack,
        ]
        .into_iter()
        .map(|f| w.protection_file_name(&span, f))
        .collect();
        assert_eq!(
            got,
            vec![
                "demo-filling-in2-back.gbr",
                "demo-capping-in2-back.gbr",
                "demo-plugging-in2.gbr",
                "demo-plugging-back.gbr",
                "demo-covering-in2.gbr",
                "demo-covering-back.gbr",
                "demo-tenting-in2.gbr",
                "demo-tenting-back.gbr",
            ]
        );
    }

    #[test]
    fn file_function_strings() {
        let b = board("demo.kicad_pcb", 4);
        let mut w = GendrillWriterBase::new(BoardCtx::bare(&b));
        let (f, b_, in1, in2) = (
            PcbLayerId::FCu,
            PcbLayerId::BCu,
            PcbLayerId::In1Cu,
            PcbLayerId::In2Cu,
        );
        let through = DrillSpan::new(f, b_, false, false);
        assert_eq!(
            w.build_file_function_attribute_string(&through, TypeFile::MixedFile, true),
            "; #@! TF.FileFunction,MixedPlating,1,4"
        );
        assert_eq!(
            w.build_file_function_attribute_string(&through, TypeFile::PthFile, false),
            "%TF.FileFunction,Plated,1,4,PTH*%"
        );
        let mut slot = hole(MM, false, HoleAttribute::HolePad, 0, 0);
        slot.is_slot = true;
        w.build_tool_list(vec![slot.clone()]);
        assert_eq!(
            w.build_file_function_attribute_string(&through, TypeFile::NpthFile, false),
            "%TF.FileFunction,NonPlated,1,4,NPTH,Rout*%"
        );
        w.build_tool_list(vec![slot, hole(MM, false, HoleAttribute::HolePad, 1, 0)]);
        assert_eq!(
            w.build_file_function_attribute_string(
                &DrillSpan::new(f, in1, false, false),
                TypeFile::PthFile,
                false
            ),
            "%TF.FileFunction,Plated,1,2,Blind,Mixed*%"
        );
        w.build_tool_list(vec![hole(MM, false, HoleAttribute::HoleViaBuried, 1, 0)]);
        assert_eq!(
            w.build_file_function_attribute_string(
                &DrillSpan::new(in1, in2, false, false),
                TypeFile::PthFile,
                false
            ),
            "%TF.FileFunction,Plated,2,3,Buried,Drill*%"
        );
        // Backdrills are non-plated and "Blind" whatever the file type; layers smaller first.
        assert_eq!(
            w.build_file_function_attribute_string(
                &DrillSpan::new(b_, in2, true, false),
                TypeFile::PthFile,
                true
            ),
            "; #@! TF.FileFunction,NonPlated,3,4,Blind"
        );
    }

    #[test]
    fn helpers() {
        assert_eq!(layer_name(PcbLayerId::FCu), "front");
        assert_eq!(layer_name(PcbLayerId::BCu), "back");
        assert_eq!(layer_name(PcbLayerId::In1Cu), "in1");
        assert_eq!(layer_name(PcbLayerId::In30Cu), "in30");
        assert_eq!(
            layer_pair_name((PcbLayerId::In2Cu, PcbLayerId::BCu)),
            "in2-back"
        );
        assert_eq!(DrillPrecision::default().precision_string(), "2:4");
        assert_eq!(DrillPrecision::new(3, 3).precision_string(), "3:3");
        assert_eq!(diameter_in_mm(300_000.0), 0.3);
        assert_eq!(diameter_in_inches(254_000.0), 0.01);
        assert_eq!(get_marker_best_pen_size(300_000), 100_000);
        assert_eq!(get_marker_best_pen_size(3_000_000), 300_000);
        assert_eq!(get_sketch_oval_best_pen_size(), 100_000);
        assert_eq!(get_default_pen_size(), 200_000);
    }

    #[test]
    fn map_files_report_unsupported_format() {
        // A map format without a plot engine in the port (FAB-12: PDF); the loop reports the
        // C++ failure message and stops.
        // Enumerating spans needs the via model when the board has vias; this board has none.
        let b = board("/p/demo.kicad_pcb", 2);
        let mut w = GendrillWriterBase::new(BoardCtx::bare(&b));
        w.drill_file_extension = "drl".to_string();
        w.set_map_file_format(PlotFormat::Pdf);
        let mut rep = crate::VecReporter::default();
        let env = crate::pcbplot::PlotEnv::default();
        let res = w.create_map_files_set("out", &env, Some(&mut rep));
        assert!(matches!(res, Err(FabError::Unsupported(_))));
        assert_eq!(
            rep.messages,
            vec![(
                "Failed to create file 'out/demo-PTH-drl_map.pdf'.".to_string(),
                Severity::Error
            )]
        );
    }
}
