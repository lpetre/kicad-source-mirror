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

//! The `Session`: the one owner object `kicad-cli` and `kicad-wasm` work with
//! (`docs/design/board-model.md` 14 and the `kicad-session` decision of section 17).
//!
//! It replaces what the C++ CLI keeps in `PCBNEW_JOBS_HANDLER::m_cliBoard` plus the project
//! the `SETTINGS_MANAGER` loaded for it: the board, the text of the sibling `.kicad_pro` and
//! `.kicad_dru` files (the host reads them; this crate never touches the filesystem) and the
//! board's file name, which the C++ uses for default output names.
//!
//! Loading is `BOARD_LOADER::Load` (`pcbnew/board_loader.cpp`): parse, then the post-load
//! initialisation (`initializeLoadedBoard`), both in [`crate::board_loader`] (package
//! PCBIO-15); [`Session::load_board_qa`] is the QA harness sequence (`KI_TEST::LoadBoard`).

use kicad_connectivity::data::ConnectivityData;
use kicad_core::PcbLayerId;
use kicad_core::layer_ids::is_copper_layer;
use kicad_core::lset::Lset;
use kicad_drc::engine::DrcEngine;
use kicad_geom::Vector2I;
use kicad_pcb::padstack::Padstack;
use kicad_pcb::pcb_track::{PcbTrack, PcbVia, Track};
use kicad_pcb::{AddMode, Board, ItemId, NetCode};
use serde::{Deserialize, Serialize};

use crate::board_loader::{LoadInputs, LoadedBoard};
use crate::error::SessionError;

/// Map an `IO_ERROR` of the board reader / writer: a staging stub of the PCBIO packages
/// ("port pending: ...") is `NotYetPorted`, anything else the C++ error text.
pub(crate) fn io_error(err: &kicad_sexpr::IoError, load: bool) -> SessionError {
    let what = err.what().to_string();

    if what.contains("port pending") {
        let side = if load { "parser" } else { "writer" };
        return SessionError::not_yet_ported(format!("kicad-pcb {side}: {what}"));
    }

    if load {
        SessionError::Load { message: what }
    } else {
        SessionError::Save { message: what }
    }
}

/// Parse the text of a `.kicad_pcb` file into a board: `PCB_IO_KICAD_SEXPR::LoadBoard`
/// (`kicad_pcb::io::parse_board` with the strict default options) plus `SetFileName`.
///
/// The one place that names the parser. A construct whose parser package has not landed yet
/// is reported as `NotYetPorted`.
pub fn parse_board_text(text: &str, source_name: &str) -> Result<Board, SessionError> {
    let options = kicad_pcb::io::ParseOptions::default();
    let mut board =
        kicad_pcb::io::parse_board(text, source_name, &options).map_err(|e| io_error(&e, true))?;
    board.file_name = source_name.to_string();
    Ok(board)
}

/// Format a board as `.kicad_pcb` text: `PCB_IO_KICAD_SEXPR::SaveBoard` without the file
/// (`kicad_pcb::io::format_board`, default options).
pub fn format_board_text(board: &mut Board) -> Result<String, SessionError> {
    let options = kicad_pcb::io::FormatOptions::default();
    kicad_pcb::io::format_board(board, &options).map_err(|e| io_error(&e, false))
}

/// Port of `PCB_IO_KICAD_SEXPR::CanReadBoard( aFileName )` on the file's name and text: the
/// extension is `kicad_pcb` (case-insensitive, `PCB_IO::CanReadBoard`) and
/// `PCB_IO_KICAD_SEXPR_PARSER::IsValidBoardHeader()` accepts the text (leading comment lines,
/// then `(kicad_pcb`). `PCB_IO_MGR::FindPluginTypeFromBoardPath` uses it to pick the plugin;
/// a file it rejects is a `Failed to load board`. Move to `kicad_pcb::io` once the parser lands.
pub fn can_read_board(file_name: &str, text: &str) -> bool {
    use kicad_sexpr::DsnLexer;
    use kicad_sexpr::keywords::pcb_keys_t::T;

    let ext = match file_full_name(file_name).rfind('.') {
        Some(dot) => file_full_name(file_name)[dot + 1..].to_lowercase(),
        None => String::new(),
    };

    if ext != "kicad_pcb" {
        return false;
    }

    let mut lexer = DsnLexer::<T>::new(text, file_name);

    // ReadCommentLines() leaves the first non-comment token current.
    if lexer.read_comment_lines().is_err() {
        return false;
    }

    if lexer.cur_tok() != T::T_LEFT {
        return false;
    }

    matches!(lexer.next_tok(), Ok(T::T_kicad_pcb))
}

/// The steps of `BOARD_LOADER::initializeLoadedBoard` the session does not run because their
/// subsystem is not ported yet. Empty: every step runs ([`crate::board_loader`]). Kept so
/// that consumers can keep checking it.
pub const PENDING_LOAD_STEPS: &[&str] = &[];

/// The board, its project and rules: what a CLI run or a browser tab works on.
pub struct Session {
    /// The board (`m_cliBoard`).
    pub board: Board,
    /// The board's file name as the host knows it (`BOARD::GetFileName()`), e.g.
    /// `"pic_programmer.kicad_pcb"` or `"/abs/dir/board.kicad_pcb"`. Default output names are
    /// derived from it.
    pub source_name: String,
    /// Text of the sibling `.kicad_pro` file, if any.
    pub project_json: Option<String>,
    /// Text of the sibling `.kicad_dru` file, if any.
    pub rules_text: Option<String>,
    /// The `initializeLoadedBoard` steps not run yet (see [`PENDING_LOAD_STEPS`]).
    pub pending_load_steps: Vec<&'static str>,
    /// `BOARD::GetConnectivity()`: built by the load (`BuildConnectivity`); empty for
    /// [`Session::new_empty`].
    pub connectivity: ConnectivityData,
    /// `BOARD_DESIGN_SETTINGS::m_DRCEngine` after the load's `InitEngine`; `None` for
    /// [`Session::new_empty`] (a bare `new BOARD()` has no engine).
    pub drc_engine: Option<DrcEngine>,
    /// The swallowed `InitEngine` error of the kicad-cli load (see
    /// [`LoadedBoard::drc_init_error`]).
    pub drc_init_error: Option<String>,
    /// Warnings the C++ loader sends to its (optional) reporter; kicad-cli prints none.
    pub load_warnings: Vec<String>,
}

/// A net as the item access API returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetInfo {
    /// `NETINFO_ITEM::GetNetCode()`.
    pub code: i32,
    /// `NETINFO_ITEM::GetNetname()`.
    pub name: String,
    /// The name of the effective netclass (`GetNetClass()->GetName()`), empty if none.
    pub netclass: String,
}

/// A track segment, arc or via as the item access API returns it (board frame, nm).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    /// The handle: `ItemId::to_bits()` (exact as a JS number).
    pub id: u64,
    /// `"track"`, `"arc"` or `"via"`.
    pub kind: String,
    /// `m_Uuid`.
    pub uuid: String,
    /// `GetStart()` (the via position for a via).
    pub start: [i32; 2],
    /// `GetEnd()`.
    pub end: [i32; 2],
    /// `GetMid()` for an arc, else `None`.
    pub mid: Option<[i32; 2]>,
    /// `m_width` for a segment / arc; the via diameter (`PADSTACK::ALL_LAYERS` size) for a via.
    pub width: i32,
    /// `GetLayer()` as the canonical layer name (`LSET::Name`); the top layer of a via.
    pub layer: String,
    /// The bottom layer of a via, else `None`.
    pub bottom_layer: Option<String>,
    /// The via drill, else `None`.
    pub drill: Option<i32>,
    /// `GetNetCode()`.
    pub net: i32,
}

/// What `addTrack` takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackSpec {
    /// Start point (nm).
    pub start: [i32; 2],
    /// End point (nm).
    pub end: [i32; 2],
    /// Width (nm).
    pub width: i32,
    /// Copper layer, canonical name (`"F.Cu"`, `"In1.Cu"`, ...).
    pub layer: String,
    /// Net code (0 = unconnected).
    #[serde(default)]
    pub net: i32,
}

/// What `addVia` takes (a through via).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViaSpec {
    /// Position (nm).
    pub at: [i32; 2],
    /// Diameter (nm).
    pub diameter: i32,
    /// Drill (nm).
    pub drill: i32,
    /// Net code (0 = unconnected).
    #[serde(default)]
    pub net: i32,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("source_name", &self.source_name)
            .field("has_project", &self.project_json.is_some())
            .field("has_rules", &self.rules_text.is_some())
            .field("pending_load_steps", &self.pending_load_steps)
            .field("drc_init_error", &self.drc_init_error)
            .field("load_warnings", &self.load_warnings)
            .finish_non_exhaustive()
    }
}

fn xy(v: Vector2I) -> [i32; 2] {
    [v.x, v.y]
}

impl Session {
    /// Port of `PCBNEW_JOBS_HANDLER::getBoard` + `BOARD_LOADER::Load` on in-memory texts:
    /// parse `text` (the `.kicad_pcb`), bind the project (`project_json`, the `.kicad_pro`;
    /// defaults without one) and the rules (`rules_text`, the `.kicad_dru`), run the post-load
    /// initialisation (`initializeLoadedBoard`, [`crate::board_loader::load`]).
    ///
    /// `source_name` is the board's file name (`BOARD::SetFileName`), used for default output
    /// names, the project name and in parser error messages; kicad-cli makes it absolute.
    pub fn load_board(
        text: &str,
        source_name: &str,
        project_json: Option<&str>,
        rules_text: Option<&str>,
    ) -> Result<Session, SessionError> {
        Session::load_board_with(&LoadInputs {
            text,
            source_name,
            project_json,
            rules_text,
            ..LoadInputs::default()
        })
    }

    /// [`Session::load_board`] with every input of the kicad-cli load (also the `.kicad_prl`
    /// text and the parser's staging switch).
    pub fn load_board_with(inputs: &LoadInputs<'_>) -> Result<Session, SessionError> {
        let loaded = crate::board_loader::load(inputs)?;
        Ok(Session::from_loaded(loaded, inputs))
    }

    /// Port of `KI_TEST::LoadBoard` (`qa/pcbnew_utils/board_test_utils.cpp`), the load
    /// sequence of the C++ QA tests and of the reference dump tool's default mode: a project
    /// only when a project file exists, `BuildListOfNets` before `BuildConnectivity`, no DRC
    /// exclusions, the board's file name not set. See [`crate::board_loader::load_qa`].
    pub fn load_board_qa(inputs: &LoadInputs<'_>) -> Result<Session, SessionError> {
        let loaded = crate::board_loader::load_qa(inputs)?;
        Ok(Session::from_loaded(loaded, inputs))
    }

    /// The session of a board loaded by [`crate::board_loader`] from `inputs`.
    pub fn from_loaded(loaded: LoadedBoard, inputs: &LoadInputs<'_>) -> Session {
        let mut session = Session {
            board: loaded.board,
            source_name: inputs.source_name.to_string(),
            project_json: inputs.project_json.map(str::to_string),
            rules_text: inputs.rules_text.map(str::to_string),
            pending_load_steps: Vec::new(),
            connectivity: loaded.connectivity,
            drc_engine: Some(loaded.drc_engine),
            drc_init_error: loaded.drc_init_error,
            load_warnings: loaded.warnings,
        };

        session.initialize_loaded_board();
        session
    }

    /// A session on a new, empty board (`new BOARD()`), for clients that build a board item by
    /// item and for tests of the item access API.
    pub fn new_empty(source_name: &str) -> Session {
        Session {
            board: Board::new(),
            source_name: source_name.to_string(),
            project_json: None,
            rules_text: None,
            pending_load_steps: Vec::new(),
            connectivity: ConnectivityData::new(),
            drc_engine: None,
            drc_init_error: None,
            load_warnings: Vec::new(),
        }
    }

    /// Records the `initializeLoadedBoard` steps that did not run (none: the sequence itself
    /// is [`crate::board_loader::initialize_loaded_board`]).
    fn initialize_loaded_board(&mut self) {
        self.pending_load_steps = PENDING_LOAD_STEPS.to_vec();
    }

    /// Applies the host-supplied values the C++ takes from the process after a load: the
    /// UUID generator seed (`boost::uuids::basic_random_generator` seeds itself from the
    /// operating system; the hosts pass OS randomness, or a fixed seed for reproducible runs)
    /// and the clock of the `CURRENT_DATE` / `CURRENT_TIME_*` text variables.
    ///
    /// The generator is reseeded in place (`KiidGenerator::seed_generator`), which keeps the
    /// reservation of the UUIDs read from the file; replacing `board.kiid_generator` would
    /// lose it.
    pub fn apply_host_inputs(&mut self, clock: &crate::HostClock, uuid_seed: u32) {
        self.board.kiid_generator.seed_generator(uuid_seed);
        self.board.text_var_clock = clock.title_block_clock();
    }

    /// Not in C++ (FONT-RC-1): one warning per text item drawn in the stroke font because its
    /// TrueType font has no usable stored glyphs (`kicad_pcb::eda_text::outline_font_fallbacks`).
    pub fn outline_font_warnings(&self) -> Vec<String> {
        let mut ctx =
            kicad_pcb::ctx::BoardCtx::bare(&self.board).with_connectivity(&self.connectivity);

        if let Some(engine) = self.drc_engine.as_ref() {
            ctx = ctx.with_rules(engine);
        }

        kicad_pcb::eda_text::outline_font_fallbacks(&ctx)
            .iter()
            .map(|f| f.message())
            .collect()
    }

    /// The board's file name without directory and extension (`wxFileName::GetName()`).
    pub fn board_stem(&self) -> String {
        file_stem(&self.source_name)
    }

    /// Port of `PCB_IO_KICAD_SEXPR::SaveBoard` to a string (`board.save()`).
    pub fn save(&mut self) -> Result<String, SessionError> {
        format_board_text(&mut self.board)
    }

    /// The nets in net code order (`BOARD::GetNetInfo()` iteration).
    pub fn nets(&self) -> Vec<NetInfo> {
        self.board
            .nets_by_netcode()
            .map(|net| NetInfo {
                code: net.net_code().0,
                name: net.netname().to_string(),
                netclass: net.netclass().map(|nc| nc.name.clone()).unwrap_or_default(),
            })
            .collect()
    }

    /// The tracks, arcs and vias in `BOARD::Tracks()` order.
    pub fn tracks(&self) -> Vec<TrackInfo> {
        self.board
            .tracks()
            .map(|(id, track)| {
                let id: ItemId = id.into();
                let t = track.as_track();
                let mut info = TrackInfo {
                    id: id.to_bits(),
                    kind: String::new(),
                    uuid: t.base.item.uuid.as_string(),
                    start: xy(t.start),
                    end: xy(t.end),
                    mid: None,
                    width: t.width,
                    layer: Lset::name(t.base.item.layer),
                    bottom_layer: None,
                    drill: None,
                    net: t.base.net.0,
                };

                match track {
                    Track::Trace(_) => info.kind = "track".to_string(),
                    Track::Arc(arc) => {
                        info.kind = "arc".to_string();
                        info.mid = Some(xy(arc.mid));
                    }
                    Track::Via(via) => {
                        info.kind = "via".to_string();
                        // PCB_VIA::GetWidth( PADSTACK::ALL_LAYERS ), GetDrillValue(),
                        // TopLayer() / BottomLayer()
                        info.width = via.padstack.size(Padstack::ALL_LAYERS).x;
                        info.drill = Some(via.padstack.drill().size.x);
                        info.layer = Lset::name(via.padstack.drill().start);
                        info.bottom_layer = Some(Lset::name(via.padstack.drill().end));
                    }
                }

                info
            })
            .collect()
    }

    fn check_net(&self, net: i32) -> Result<NetCode, SessionError> {
        let code = NetCode(net);

        if self.board.find_net(code).is_none() {
            return Err(SessionError::invalid_argument(format!(
                "no net with code {net}"
            )));
        }

        Ok(code)
    }

    /// Add a straight track segment (`new PCB_TRACK( board )`, `SetStart/SetEnd/SetWidth/
    /// SetLayer/SetNetCode`, `BOARD::Add( track, ADD_MODE::APPEND )`); returns its handle.
    pub fn add_track(&mut self, spec: &TrackSpec) -> Result<u64, SessionError> {
        let layer_id = Lset::name_to_layer(&spec.layer);

        if layer_id < 0 {
            return Err(SessionError::invalid_argument(format!(
                "unknown layer '{}'",
                spec.layer
            )));
        }

        let layer = PcbLayerId::from(layer_id);

        if !is_copper_layer(layer) {
            // BOARD::Add refuses it with the same message.
            return Err(SessionError::invalid_argument(
                "Cannot place Track on non-copper layer",
            ));
        }

        let net = self.check_net(spec.net)?;

        let mut track = PcbTrack::new(&mut self.board.kiid_generator);
        track.start = Vector2I::new(spec.start[0], spec.start[1]);
        track.end = Vector2I::new(spec.end[0], spec.end[1]);
        track.width = spec.width;
        track.base.item.layer = layer;
        track.base.net = net;

        match self.board.add_track(Track::Trace(track), AddMode::Append) {
            Ok(id) => Ok(ItemId::from(id).to_bits()),
            Err(_) => Err(SessionError::invalid_argument(
                "Cannot place Track on non-copper layer",
            )),
        }
    }

    /// Add a through via (`new PCB_VIA( board )`, `SetPosition`, `SetWidth( d, ALL_LAYERS )`,
    /// `SetDrill`, `SetNetCode`, `BOARD::Add( via, ADD_MODE::APPEND )`); returns its handle.
    pub fn add_via(&mut self, spec: &ViaSpec) -> Result<u64, SessionError> {
        let net = self.check_net(spec.net)?;
        let pos = Vector2I::new(spec.at[0], spec.at[1]);

        let mut via = PcbVia::new(&mut self.board.kiid_generator);
        // PCB_VIA::SetPosition: m_Start = m_End = aPoint
        via.track.start = pos;
        via.track.end = pos;
        // PCB_VIA::SetWidth( aWidth, aLayer ): m_padStack.SetSize( { aWidth, aWidth }, aLayer )
        via.padstack.set_size(
            Vector2I::new(spec.diameter, spec.diameter),
            Padstack::ALL_LAYERS,
        );
        // PCB_VIA::SetDrill( aDrill ): m_padStack.Drill().size = { aDrill, aDrill }
        via.padstack.drill_mut().size = Vector2I::new(spec.drill, spec.drill);
        via.track.base.net = net;

        match self
            .board
            .add_track(Track::Via(Box::new(via)), AddMode::Append)
        {
            Ok(id) => Ok(ItemId::from(id).to_bits()),
            Err(_) => Err(SessionError::invalid_argument("via refused by BOARD::Add")),
        }
    }

    /// `BOARD::Remove` for the item with handle `id`; false if no live item has it.
    pub fn remove(&mut self, id: u64) -> Result<bool, SessionError> {
        let Some(item) = ItemId::from_bits(id) else {
            return Err(SessionError::StaleId { id });
        };

        Ok(self.board.remove(item))
    }

    /// `CONNECTIVITY_DATA::GetRatsnest` after `RecalculateRatsnest` (`board.ratsnest()`).
    pub fn ratsnest(&mut self) -> Result<Vec<[i32; 4]>, SessionError> {
        Err(SessionError::not_yet_ported(
            "kicad-connectivity: CONNECTIVITY_DATA::Build / RecalculateRatsnest",
        ))
    }

    /// `ZONE_FILLER_TOOL::FillAllZones( nullptr, reporter, true )` (`board.refillZones()`):
    /// see [`Session::fill_all_zones`].
    pub fn refill_zones(&mut self) -> Result<(), SessionError> {
        self.fill_all_zones().map(|_| ())
    }

    /// Port of the headless `ZONE_FILLER_TOOL::FillAllZones` the CLI jobs call
    /// (`kicad_zone::fill_all_zones`: teardrop update, fill, connectivity rebuild), with the
    /// filler's warnings. A board without a DRC engine ([`Session::new_empty`]) first gets one,
    /// initialised from the session's rule text, as `ZONE_FILLER::Fill` does (rule errors are
    /// swallowed there).
    pub fn fill_all_zones(&mut self) -> Result<kicad_zone::FillAllZonesOutcome, SessionError> {
        if self.drc_engine.is_none() {
            let mut engine = DrcEngine::new();
            let rules_path = crate::board_loader::with_extension(&self.source_name, "kicad_dru");
            let _ = engine.init_engine(
                &mut self.board,
                self.rules_text.as_deref().map(|t| (t, rules_path.as_str())),
                kicad_drc::rule_parser::default_layer_choices(),
                &kicad_drc::engine::implicit_rules::TuningProfiles::default(),
                None,
            );
            self.drc_engine = Some(engine);
        }

        let Some(engine) = self.drc_engine.as_ref() else {
            return Err(SessionError::not_yet_ported("DRC engine"));
        };

        Ok(kicad_zone::fill_all_zones(
            &mut self.board,
            &mut self.connectivity,
            engine,
        ))
    }
}

/// `wxFileName( aPath ).GetName()` for `/` or `\` separated paths.
pub fn file_stem(path: &str) -> String {
    let name = file_full_name(path);

    match name.rfind('.') {
        Some(0) | None => name.to_string(),
        Some(dot) => name[..dot].to_string(),
    }
}

/// `wxFileName( aPath ).GetFullName()`: the part after the last separator.
pub fn file_full_name(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(sep) => &path[sep + 1..],
        None => path,
    }
}

/// `wxFileName( aPath ).GetPath()` with a trailing separator removed: the directory part, empty
/// when there is none.
pub fn file_dir(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(sep) => &path[..sep],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_reports_bad_board() {
        let err = Session::load_board("(kicad_pcb", "x.kicad_pcb", None, None).unwrap_err();
        assert!(
            matches!(
                err,
                SessionError::Load { .. } | SessionError::NotYetPorted { .. }
            ),
            "{err}"
        );
    }

    #[test]
    fn file_name_helpers() {
        assert_eq!(file_stem("/a/b/pic_programmer.kicad_pcb"), "pic_programmer");
        assert_eq!(file_stem("board.v2.kicad_pcb"), "board.v2");
        assert_eq!(file_stem("noext"), "noext");
        assert_eq!(file_full_name("a\\b.kicad_pcb"), "b.kicad_pcb");
        assert_eq!(file_dir("/a/b/c.kicad_pcb"), "/a/b");
        assert_eq!(file_dir("c.kicad_pcb"), "");
    }

    #[test]
    fn item_access_on_empty_board() {
        let mut s = Session::new_empty("empty.kicad_pcb");
        let nets = s.nets();
        assert!(nets.iter().any(|n| n.code == 0), "{nets:?}");
        assert!(s.tracks().is_empty());

        let t = s
            .add_track(&TrackSpec {
                start: [0, 0],
                end: [1_000_000, 0],
                width: 250_000,
                layer: "F.Cu".to_string(),
                net: 0,
            })
            .unwrap();
        let v = s
            .add_via(&ViaSpec {
                at: [1_000_000, 0],
                diameter: 600_000,
                drill: 300_000,
                net: 0,
            })
            .unwrap();

        let tracks = s.tracks();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].id, t);
        assert_eq!(tracks[0].kind, "track");
        assert_eq!(tracks[0].layer, "F.Cu");
        assert_eq!(tracks[1].id, v);
        assert_eq!(tracks[1].kind, "via");
        assert_eq!(tracks[1].width, 600_000);
        assert_eq!(tracks[1].drill, Some(300_000));
        assert_eq!(tracks[1].layer, "F.Cu");
        assert_eq!(tracks[1].bottom_layer.as_deref(), Some("B.Cu"));

        let err = s
            .add_track(&TrackSpec {
                start: [0, 0],
                end: [1, 1],
                width: 1,
                layer: "F.SilkS".to_string(),
                net: 0,
            })
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid argument: Cannot place Track on non-copper layer"
        );
        assert!(
            s.add_track(&TrackSpec {
                layer: "Nope".into(),
                ..tspec()
            })
            .is_err()
        );
        assert!(s.add_track(&TrackSpec { net: 99, ..tspec() }).is_err());

        assert!(s.remove(t).unwrap());
        assert!(!s.remove(t).unwrap());
        assert_eq!(s.tracks().len(), 1);
        assert!(matches!(
            s.remove(1 << 60),
            Err(SessionError::StaleId { .. })
        ));
    }

    fn tspec() -> TrackSpec {
        TrackSpec {
            start: [0, 0],
            end: [1, 1],
            width: 1,
            layer: "B.Cu".to_string(),
            net: 0,
        }
    }
}
