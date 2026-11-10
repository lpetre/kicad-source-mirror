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

//! The JavaScript API of the KiCad Rust port (new code; `README.md` documents the surface).
//!
//! A thin `wasm-bindgen` layer: arguments are converted with `serde-wasm-bindgen`, the work is
//! done by [`api`] (target independent, tested natively) and `kicad_session` (the board
//! session and the jobs). Errors are JavaScript `Error` objects whose `name` is
//! `NotYetPortedError` (with a `subsystem` property), `LoadError`, `InvalidArgumentError` or
//! `StaleIdError`.

pub mod api;

use js_sys::{Error as JsError, Reflect};
use kicad_session::jobs::{
    DrillFormat, DrillUnits, JobExportPcbDrill, JobExportPcbGerbers, JobExportPcbSvg, JobPcbDrc,
    RcOutputFormat, RcUnits, SvgGenMode,
};
use kicad_session::{
    JobOutcome, Reporter, Session, SessionError, TrackSpec, ViaSpec, run_drc, run_export_drill,
    run_export_gerbers, run_export_svg,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Module start: route Rust panics (e.g. a `port_pending!` stub) to `console.error`.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_large_number_types_as_bigints(false);
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from(JsError::new(&e.to_string())))
}

fn from_js<T: for<'de> Deserialize<'de> + Default>(value: JsValue) -> Result<T, JsValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(T::default());
    }

    serde_wasm_bindgen::from_value(value)
        .map_err(|e| session_error(SessionError::invalid_argument(e.to_string())))
}

/// A JavaScript `Error` for a session error: `name` = `<kind>Error`, plus `subsystem` for
/// `NotYetPortedError`.
fn session_error(err: SessionError) -> JsValue {
    let js = JsError::new(&err.to_string());
    js.set_name(&format!("{}Error", err.kind()));

    if let SessionError::NotYetPorted { subsystem } = &err {
        let _ = Reflect::set(
            &js,
            &JsValue::from_str("subsystem"),
            &JsValue::from_str(subsystem),
        );
    }

    js.into()
}

fn plain_error(message: &str) -> JsValue {
    JsError::new(message).into()
}

/// `version()`: `"10.99.0"`.
#[wasm_bindgen]
pub fn version() -> String {
    api::version()
}

/// `versionInfo()`: `{ version, build, commit }`.
#[wasm_bindgen(js_name = versionInfo)]
pub fn version_info() -> Result<JsValue, JsValue> {
    to_js(&api::version_info())
}

/// `prettify(text)`: the KiCad s-expression pretty printer.
#[wasm_bindgen]
pub fn prettify(text: &str) -> String {
    api::prettify(text)
}

/// `tokenStats(text, name?)`: lexer token statistics.
#[wasm_bindgen(js_name = tokenStats)]
pub fn token_stats(text: &str, name: Option<String>) -> Result<JsValue, JsValue> {
    let name = name.unwrap_or_else(|| "input".to_string());
    let stats = api::token_stats(text, &name).map_err(|e| plain_error(&e))?;
    to_js(&stats)
}

/// `strokeText(text, sizeMm)`: `{ path, strokeWidth, segments }` (SVG path data in nm).
#[wasm_bindgen(js_name = strokeText)]
pub fn stroke_text(text: &str, size_mm: f64) -> Result<JsValue, JsValue> {
    to_js(&api::stroke_text(text, size_mm))
}

/// `polygonBoolean(a, b, op)`: polygons are arrays of rings of `[x, y]` (ring 0 the outline).
#[wasm_bindgen(js_name = polygonBoolean)]
pub fn polygon_boolean(a: JsValue, b: JsValue, op: &str) -> Result<JsValue, JsValue> {
    let a: Vec<api::JsPolygon> = from_js(a)?;
    let b: Vec<api::JsPolygon> = from_js(b)?;
    let result = api::polygon_boolean(&a, &b, op).map_err(|e| plain_error(&e))?;
    to_js(&result)
}

/// `plotDemoGerber(date?)`: a small Gerber file from the Rust Gerber plotter.
#[wasm_bindgen(js_name = plotDemoGerber)]
pub fn plot_demo_gerber(date: Option<String>) -> String {
    let date = date.unwrap_or_else(|| "2026-01-01 00:00:00".to_string());
    api::plot_demo_gerber(kicad_session::build_version::build_version(), &date)
}

/// Options of `loadBoard`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct LoadOptions {
    /// Text of the `.kicad_pro`.
    pub project: Option<String>,
    /// Text of the `.kicad_dru`.
    pub rules: Option<String>,
    /// The board's file name (default `board.kicad_pcb`).
    pub name: Option<String>,
}

/// `loadBoard(text, { project, rules, name })`.
#[wasm_bindgen(js_name = loadBoard)]
pub fn load_board(text: &str, options: JsValue) -> Result<Board, JsValue> {
    let opts: LoadOptions = from_js(options)?;
    let name = opts.name.unwrap_or_else(|| "board.kicad_pcb".to_string());
    let session = Session::load_board(text, &name, opts.project.as_deref(), opts.rules.as_deref())
        .map_err(session_error)?;
    Ok(Board { session })
}

/// `newBoard(name?)`: an empty board (`new BOARD()`), for building a board item by item.
#[wasm_bindgen(js_name = newBoard)]
pub fn new_board(name: Option<String>) -> Board {
    Board {
        session: Session::new_empty(&name.unwrap_or_else(|| "board.kicad_pcb".to_string())),
    }
}

/// Options of `runDrc` (`kicad-cli pcb drc` flags).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DrcOptions {
    /// `"mm"` (default), `"in"`, `"mils"`.
    pub units: Option<String>,
    /// `"report"` (default) or `"json"`.
    pub format: Option<String>,
    /// Severity mask bits as in `--severity-*` (`error`, `warning`, `exclusions`); empty =
    /// errors and warnings.
    pub severities: Vec<String>,
    /// `--all-track-errors`
    pub all_track_errors: bool,
    /// `--refill-zones`
    pub refill_zones: bool,
}

/// Options of `exportGerbers` (`kicad-cli pcb export gerbers` flags).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GerberOptions {
    /// `--layers`
    pub layers: Option<String>,
    /// `--common-layers`
    pub common_layers: Option<String>,
    /// `--precision` (default 6)
    pub precision: Option<i32>,
    /// `--no-x2`
    pub no_x2: bool,
    /// `--no-netlist`
    pub no_netlist: bool,
    /// `--no-protel-ext`
    pub no_protel_ext: bool,
    /// `--disable-aperture-macros`
    pub disable_aperture_macros: bool,
    /// `--use-drill-file-origin`
    pub use_drill_file_origin: bool,
    /// `--subtract-soldermask`
    pub subtract_soldermask: bool,
    /// `--board-plot-params`
    pub board_plot_params: bool,
    /// `--check-zones`
    pub check_zones: bool,
}

/// Options of `exportDrill` (`kicad-cli pcb export drill` flags).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DrillOptions {
    /// `"excellon"` (default) or `"gerber"`.
    pub format: Option<String>,
    /// `"mm"` (default) or `"in"`.
    pub units: Option<String>,
    /// `--excellon-separate-th`
    pub separate_th: bool,
    /// `--generate-report`
    pub generate_report: bool,
}

/// Options of `plotSvg` (`kicad-cli pcb export svg --mode-single --exclude-drawing-sheet`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SvgOptions {
    /// `--layers` (default `"F.Cu,B.Cu,Edge.Cuts"`).
    pub layers: Option<String>,
    /// `--fit-page-to-board`
    pub fit_page_to_board: bool,
    /// `--black-and-white`
    pub black_and_white: bool,
}

/// A produced file for JavaScript (`content` as text; Gerber, Excellon and reports are ASCII).
#[derive(Serialize)]
struct JsFile {
    name: String,
    content: String,
}

/// The result of a job for JavaScript.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsJobResult {
    exit_code: i32,
    files: Vec<JsFile>,
    report: Option<String>,
    messages: Vec<kicad_session::ReportLine>,
}

fn job_result(outcome: JobOutcome, reporter: Reporter) -> Result<JsValue, JsValue> {
    to_js(&JsJobResult {
        exit_code: outcome.exit_code,
        files: outcome
            .files
            .into_iter()
            .map(|f| JsFile {
                name: f.name,
                content: String::from_utf8_lossy(&f.contents).into_owned(),
            })
            .collect(),
        report: outcome.report_text,
        messages: reporter.into_lines(),
    })
}

/// A loaded board: a JavaScript class wrapping a `kicad_session::Session`. Item handles are
/// `ItemId::to_bits()` numbers, valid for this board object.
#[wasm_bindgen]
pub struct Board {
    session: Session,
}

#[wasm_bindgen]
impl Board {
    /// The board's file name.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.session.source_name.clone()
    }

    /// `runDrc(options)`: `{ exitCode, files, report, messages }`.
    #[wasm_bindgen(js_name = runDrc)]
    pub fn run_drc(&mut self, options: JsValue) -> Result<JsValue, JsValue> {
        let o: DrcOptions = from_js(options)?;
        let mut job = JobPcbDrc::new();
        job.rc.exit_code_violations = false;
        job.parity = false;
        job.rc.units = match o.units.as_deref() {
            None | Some("mm") => RcUnits::Mm,
            Some("in") => RcUnits::Inch,
            Some("mils") => RcUnits::Mils,
            Some(u) => {
                return Err(session_error(SessionError::invalid_argument(format!(
                    "Invalid units specified: {u}"
                ))));
            }
        };
        job.rc.format = match o.format.as_deref() {
            None | Some("report") => RcOutputFormat::Report,
            Some("json") => RcOutputFormat::Json,
            Some(f) => {
                return Err(session_error(SessionError::invalid_argument(format!(
                    "Invalid report format: {f}"
                ))));
            }
        };

        let mut severity = 0;

        for s in &o.severities {
            severity |= match s.as_str() {
                "error" => kicad_core::Severity::Error.as_i32(),
                "warning" => kicad_core::Severity::Warning.as_i32(),
                "exclusions" => kicad_core::Severity::Exclusion.as_i32(),
                _ => 0,
            };
        }

        if severity != 0 {
            job.rc.severity = severity;
        }

        job.report_all_track_errors = o.all_track_errors;
        job.refill_zones = o.refill_zones;

        let mut reporter = Reporter::new();
        let outcome = run_drc(&mut self.session, &mut job, &mut reporter).map_err(session_error)?;
        job_result(outcome, reporter)
    }

    /// `exportGerbers(options)`: `{ exitCode, files: [{ name, content }], messages }`.
    #[wasm_bindgen(js_name = exportGerbers)]
    pub fn export_gerbers(&mut self, options: JsValue) -> Result<JsValue, JsValue> {
        let o: GerberOptions = from_js(options)?;
        let mut job = JobExportPcbGerbers::new();
        // the kicad-cli values (command_pcb_export_gerber.cpp populateJob)
        job.gerber.plot.sketch_dnp_fps_on_fab_layers = false;
        job.gerber.plot.crossout_dnp_fps_on_fab_layers = false;
        job.gerber.plot.arg_layers = Some(o.layers.unwrap_or_default());
        job.gerber.plot.arg_common_layers = Some(o.common_layers.unwrap_or_default());
        job.gerber.precision = o.precision.unwrap_or(6);
        job.gerber.use_x2_format = !o.no_x2;
        job.gerber.include_netlist_attributes = !o.no_netlist;
        job.gerber.use_protel_file_extension = !o.no_protel_ext;
        job.gerber.disable_aperture_macros = o.disable_aperture_macros;
        job.gerber.plot.use_drill_origin = o.use_drill_file_origin;
        job.gerber.plot.subtract_solder_mask_from_silk = o.subtract_soldermask;
        job.gerber.plot.check_zones_before_plot = o.check_zones;
        job.use_board_plot_params = o.board_plot_params;

        if job.gerber.precision != 5 && job.gerber.precision != 6 {
            return Err(session_error(SessionError::invalid_argument(
                "Gerber coordinate precision should be either 5 or 6",
            )));
        }

        let mut reporter = Reporter::new();
        let outcome = run_export_gerbers(&mut self.session, &mut job, &mut reporter)
            .map_err(session_error)?;
        job_result(outcome, reporter)
    }

    /// `exportDrill(options)`: `{ exitCode, files, report, messages }`.
    #[wasm_bindgen(js_name = exportDrill)]
    pub fn export_drill(&mut self, options: JsValue) -> Result<JsValue, JsValue> {
        let o: DrillOptions = from_js(options)?;
        let mut job = JobExportPcbDrill::new();
        job.format = match o.format.as_deref() {
            None | Some("excellon") => DrillFormat::Excellon,
            Some("gerber") => DrillFormat::Gerber,
            Some(f) => {
                return Err(session_error(SessionError::invalid_argument(format!(
                    "Invalid drill format: {f}"
                ))));
            }
        };
        job.drill_units = match o.units.as_deref() {
            None | Some("mm") => DrillUnits::Mm,
            Some("in") => DrillUnits::Inch,
            Some(u) => {
                return Err(session_error(SessionError::invalid_argument(format!(
                    "Invalid units specified: {u}"
                ))));
            }
        };
        job.excellon_combine_pth_npth = !o.separate_th;
        job.generate_report = o.generate_report;

        let mut reporter = Reporter::new();
        let outcome =
            run_export_drill(&mut self.session, &mut job, &mut reporter).map_err(session_error)?;
        job_result(outcome, reporter)
    }

    /// `plotSvg(options)`: the SVG text of a single-file plot.
    #[wasm_bindgen(js_name = plotSvg)]
    pub fn plot_svg(&mut self, options: JsValue) -> Result<String, JsValue> {
        let o: SvgOptions = from_js(options)?;
        let mut job = JobExportPcbSvg::new();
        job.gen_mode = SvgGenMode::Single;
        job.plot.plot_drawing_sheet = false;
        job.plot.sketch_dnp_fps_on_fab_layers = false;
        job.plot.crossout_dnp_fps_on_fab_layers = false;
        job.plot.arg_layers = Some(
            o.layers
                .unwrap_or_else(|| "F.Cu,B.Cu,Edge.Cuts".to_string()),
        );
        job.plot.arg_common_layers = Some(String::new());
        job.fit_page_to_board = o.fit_page_to_board;
        job.plot.black_and_white = o.black_and_white;

        let mut reporter = Reporter::new();
        let outcome =
            run_export_svg(&mut self.session, &mut job, &mut reporter).map_err(session_error)?;

        match outcome.files.into_iter().next() {
            Some(f) => Ok(String::from_utf8_lossy(&f.contents).into_owned()),
            None => Err(plain_error(&reporter.text())),
        }
    }

    /// `save()`: the board as `.kicad_pcb` text.
    pub fn save(&mut self) -> Result<String, JsValue> {
        self.session.save().map_err(session_error)
    }

    /// `ratsnest()`: `Int32Array` of `[x1, y1, x2, y2] * n`.
    pub fn ratsnest(&mut self) -> Result<Vec<i32>, JsValue> {
        let lines = self.session.ratsnest().map_err(session_error)?;
        Ok(lines.into_iter().flatten().collect())
    }

    /// `refillZones()`.
    #[wasm_bindgen(js_name = refillZones)]
    pub fn refill_zones(&mut self) -> Result<(), JsValue> {
        self.session.refill_zones().map_err(session_error)
    }

    /// `nets()`: `[{ code, name, netclass }]` in net code order.
    pub fn nets(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.nets())
    }

    /// `tracks()`: `[{ id, kind, uuid, start, end, mid, width, layer, bottomLayer, drill, net }]`.
    pub fn tracks(&self) -> Result<JsValue, JsValue> {
        to_js(&self.session.tracks())
    }

    /// `addTrack({ start: [x, y], end: [x, y], width, layer, net })` -> id.
    #[wasm_bindgen(js_name = addTrack)]
    pub fn add_track(&mut self, spec: JsValue) -> Result<f64, JsValue> {
        let spec: TrackSpec = serde_wasm_bindgen::from_value(spec)
            .map_err(|e| session_error(SessionError::invalid_argument(e.to_string())))?;
        let id = self.session.add_track(&spec).map_err(session_error)?;
        Ok(id as f64)
    }

    /// `addVia({ at: [x, y], diameter, drill, net })` -> id.
    #[wasm_bindgen(js_name = addVia)]
    pub fn add_via(&mut self, spec: JsValue) -> Result<f64, JsValue> {
        let spec: ViaSpec = serde_wasm_bindgen::from_value(spec)
            .map_err(|e| session_error(SessionError::invalid_argument(e.to_string())))?;
        let id = self.session.add_via(&spec).map_err(session_error)?;
        Ok(id as f64)
    }

    /// `remove(id)`: true if the item existed.
    pub fn remove(&mut self, id: f64) -> Result<bool, JsValue> {
        if !(id >= 0.0 && id.fract() == 0.0 && id < 9_007_199_254_740_992.0) {
            return Err(session_error(SessionError::StaleId { id: 0 }));
        }

        self.session.remove(id as u64).map_err(session_error)
    }
}
