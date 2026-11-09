// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.h` and of the helper part of
//! `pcb_io_kicad_sexpr_parser.cpp` (`init`, `skipCurrent`, `pushValueIntoMap`,
//! `parseBoardUnits`, `parseBool` and its variants, `parseNet`, `GetRequiredVersion`,
//! `parseXY`, `parseOutlinePoints`, `parseMargins`, `parseBoardProperty`,
//! `parseCustomProperty`, the layer lookups `lookUpLayer` / `lookUpLayerSet` /
//! `parseBoardItemLayer` / `parseBoardItemLayersAsMask` / `parseLayersForCuItemWithSoldermask`,
//! `parseFrontBackOptBool`, `CurStrToKIID`): package PCBIO-01.
//!
//! The parser is a pull parser over the `PCB_LEXER` token stream exactly like the C++
//! (`docs/analysis/pcb-io.md` 5.1): [`PcbParser::lexer`] is a `DsnLexer` with the generated
//! `PCB_KEYS_T` table, so the token classes (keyword vs. quoted string vs. number vs. symbol)
//! and `PrevTok()` are those of the C++.
//!
//! One file per construct group, each owned by its work package (`pcb-io.md` 10.2):
//!
//! | file | package | C++ |
//! |---|---|---|
//! | `board.rs` | PCBIO-01 | `Parse`, `finishItem`, `parseBOARD*`, header, general, page, title block, layers, net declarations |
//! | `tracks.rs`, `shapes.rs` | PCBIO-03 | tracks, arcs, vias, teardrop parameters / shapes, line endings |
//! | `setup.rs` | PCBIO-04 | setup, stackup, defaults, drill symbol profile, plot parameters |
//! | `text.rs` | PCBIO-05 | effects, render cache, text, text box |
//! | `footprint.rs` | PCBIO-06 | footprint, 3D model, footprint stackup / variant |
//! | `pad.rs` | PCBIO-07 | pad, primitives, options, post machining, padstack |
//! | `zone.rs` | PCBIO-08 | zone, zone defaults / layer properties |
//! | `group.rs` | PCBIO-09 | group, `resolveGroups` |
//! | `dimension.rs` | PCBIO-10 | dimension |
//! | `netclass_embedded.rs` | PCBIO-11 | variants, legacy net classes, net chains, embedded files / fonts |
//! | `misc_items.rs` | PCBIO-12 | reference image, barcode, tables, generated tables, drill map, target, point, grid item |
//! | `generator.rs` | PCBIO-13 | generator, constraint, `resolveConstraints` |
//! | `fp_lib.rs` | PCBIO-14 | `ParseFootprintLib` |
//!
//! Until a package lands, its functions are **staging stubs** ([`PcbParser::pending`]): with
//! `ParseOptions::allow_pending == false` they fail with "port pending: <token> (PCBIO-nn)",
//! with `allow_pending == true` they skip the balanced s-expression and record it in
//! [`ParseReport::skipped`].

// The helper API and the staging stubs are consumed by the PCBIO packages as they land.
#![allow(dead_code)]

mod board;
mod dimension;
mod footprint;
mod fp_lib;
mod generator;
mod group;
mod misc_items;
mod netclass_embedded;
mod pad;
mod setup;
mod shapes;
mod text;
mod tracks;
mod zone;

pub use board::ParsedItem;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kicad_core::lib_id::LibId;
use kicad_core::{EdaDataType, Kiid, Lset, PCB_IU_SCALE, PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_geom::{ShapeArc, ShapeLineChain, Vector2I, ki_round};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::{PcbLexer, T};

use crate::board::Board;
use crate::ids::NetCode;
use crate::item::ParentRef;
use crate::item_ref::OwnedItem;
use crate::pcb_constraint::{ConstraintMember, PcbConstraintType};
use crate::pcb_generator::GeneratorProp;
use crate::traits::BoardItem;

/// `INT_LIMIT`: board units are integers; values beyond this are clamped by `parseBoardUnits`.
/// Slightly smaller than `INT_MAX` to avoid issues in comparisons.
pub const INT_LIMIT: f64 = (i32::MAX - 10) as f64;

/// Options of a parse.
#[derive(Clone, Debug, Default)]
pub struct ParseOptions {
    /// Staging switch while the PCBIO packages land: when `true`, a construct whose parser is
    /// still a staging stub is skipped (balanced s-expression) and recorded in
    /// [`ParseReport::skipped`] instead of failing the parse with "port pending: ...".
    pub allow_pending: bool,
}

/// One construct skipped by a staging stub (`ParseOptions::allow_pending`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedItem {
    /// The keyword of the skipped list, e.g. `"segment"`.
    pub token: String,
    /// Line of the keyword (`CurLineNumber()`).
    pub line: i32,
    /// The package that will port it, e.g. `"PCBIO-03"`.
    pub package: &'static str,
}

/// What a parse reports next to the board.
#[derive(Clone, Debug, Default)]
pub struct ParseReport {
    /// `GetParseWarnings()`: non-fatal warnings (embedded-file errors, zero-sized pads, legacy
    /// zone fills, ...), in the order the C++ pushes them.
    pub warnings: Vec<String>,
    /// Constructs skipped by staging stubs, in file order.
    pub skipped: Vec<SkippedItem>,
}

/// `GROUP_INFO`: a group declaration, resolved after the file is read (`resolveGroups`).
#[derive(Clone, Debug, Default)]
pub struct GroupInfo {
    /// `parent`: the board (`ParentRef::Board`) or the footprint being parsed.
    pub parent: ParentRef,
    pub name: String,
    pub locked: bool,
    pub uuid: Kiid,
    pub lib_id: LibId,
    /// `memberUuids`, file order.
    pub member_uuids: Vec<Kiid>,
    pub custom_properties: BTreeMap<String, String>,
}

/// `GENERATOR_INFO : GROUP_INFO`.
#[derive(Clone, Debug, Default)]
pub struct GeneratorInfo {
    pub group: GroupInfo,
    pub layer: PcbLayerId,
    /// `genType`.
    pub gen_type: String,
    /// `properties` (`STRING_ANY_MAP`, ordered by key).
    pub properties: BTreeMap<String, GeneratorProp>,
    /// Named template items parsed from the generator's `(templates ...)` section: settings
    /// carriers owned by the generator, not on the board.
    pub templates: Vec<(String, OwnedItem)>,
}

/// `CONSTRAINT_INFO`: a deferred constraint, resolved once the whole file is read.
#[derive(Clone, Debug)]
pub struct ConstraintInfo {
    pub parent: ParentRef,
    pub uuid: Kiid,
    pub type_: PcbConstraintType,
    pub members: Vec<ConstraintMember>,
    pub value: Option<f64>,
    pub driving: bool,
    pub custom_properties: BTreeMap<String, String>,
}

/// Port of `PCB_IO_KICAD_SEXPR_PARSER`: reads a Pcbnew s-expression text and fills a board.
///
/// The C++ parser holds a `BOARD*` that is null while a library footprint is parsed; here the
/// parser always owns a [`Board`] (a holder board for footprint files, `board-model.md` 2) and
/// [`board_present`](Self::board_present) reproduces the `m_board != nullptr` tests.
pub struct PcbParser {
    /// The `PCB_LEXER` base class.
    pub lexer: PcbLexer,
    /// `m_board` (the board being filled).
    pub board: Board,
    /// `m_board != nullptr`.
    pub board_present: bool,
    pub(crate) options: ParseOptions,
    /// `m_layerIndices`: layer name -> id (lookup only).
    pub(crate) layer_indices: HashMap<String, PcbLayerId>,
    /// `m_layerMasks`: layer name or wildcard -> set (lookup only).
    pub(crate) layer_masks: HashMap<String, Lset>,
    /// `m_undefinedLayers`: names not found by `lookUpLayer` (ordered, as `std::set<wxString>`).
    pub(crate) undefined_layers: BTreeSet<String>,
    /// `m_netCodes`: legacy file net code -> board net code.
    pub(crate) net_codes: Vec<i32>,
    /// `m_tooRecent`.
    pub(crate) too_recent: bool,
    /// `m_requiredVersion`: the file format version (0 until a version is seen).
    pub(crate) required_version: i32,
    /// `m_generatorVersion`.
    pub(crate) generator_version: String,
    /// `m_appendToExisting`: always false (append mode is PCBIO-16).
    pub(crate) append_to_existing: bool,
    /// `m_preserveDestinationStackup`: always false (append mode is PCBIO-16).
    pub(crate) preserve_destination_stackup: bool,
    /// `m_resetKIIDMap`: only filled in append mode.
    pub(crate) reset_kiid_map: HashMap<String, Kiid>,
    /// `m_linkedZones`: zones read with a link set or anchor.
    pub(crate) linked_zones: BTreeSet<Kiid>,
    pub(crate) show_legacy_segment_zone_warning: bool,
    pub(crate) show_legacy5_zone_warning: bool,
    /// `m_groupInfos`, `m_generatorInfos`, `m_constraintInfos`: deferred declarations, cleared
    /// after every top-level item (`finishItem`).
    pub(crate) group_infos: Vec<GroupInfo>,
    pub(crate) generator_infos: Vec<GeneratorInfo>,
    pub(crate) constraint_infos: Vec<ConstraintInfo>,
    /// `m_parseWarnings` and the staging-stub skips.
    pub(crate) report: ParseReport,
}

impl PcbParser {
    /// Port of the constructor `PCB_IO_KICAD_SEXPR_PARSER( aReader, aBoard, ... )` (no query
    /// callback, no progress reporter, no preserved stackup, no append) followed by `init()`.
    ///
    /// `source` is the line reader's source name used in error messages; `board_present`
    /// says whether the C++ parser would have a board (`false` for library footprints).
    pub fn new(
        text: &str,
        source: &str,
        board: Board,
        board_present: bool,
        options: ParseOptions,
    ) -> Self {
        // STRING_LINE_READER keeps an empty source name (QA's STDISTREAM_LINE_READER has
        // none either); DsnLexer::new would substitute "clipboard".
        let reader = kicad_sexpr::StringLineReader::new(text, source);
        let lexer = PcbLexer::from_line_reader(Some(Box::new(reader)));

        let mut parser = PcbParser {
            lexer,
            board,
            board_present,
            options,
            layer_indices: HashMap::new(),
            layer_masks: HashMap::new(),
            undefined_layers: BTreeSet::new(),
            net_codes: Vec::new(),
            too_recent: false,
            required_version: 0,
            generator_version: String::new(),
            append_to_existing: false,
            preserve_destination_stackup: false,
            reset_kiid_map: HashMap::new(),
            linked_zones: BTreeSet::new(),
            show_legacy_segment_zone_warning: true,
            show_legacy5_zone_warning: true,
            group_infos: Vec::new(),
            generator_infos: Vec::new(),
            constraint_infos: Vec::new(),
            report: ParseReport::default(),
        };

        parser.init();
        parser
    }

    /// Port of `PCB_IO_KICAD_SEXPR_PARSER::init`: reset the state and seed the layer maps with
    /// the untranslated default (English) layer names and the wildcards.
    pub(crate) fn init(&mut self) {
        self.show_legacy_segment_zone_warning = true;
        self.show_legacy5_zone_warning = true;
        self.too_recent = false;
        self.required_version = 0;
        self.layer_indices.clear();
        self.layer_masks.clear();
        self.reset_kiid_map.clear();
        self.linked_zones.clear();

        // Add untranslated default (i.e. English) layernames.
        // Some may be overridden later if parsing a board rather than a footprint.
        // The English name will survive if parsing only a footprint.
        for layer in 0..PCB_LAYER_ID_COUNT {
            let untranslated = Lset::name(PcbLayerId(layer));

            self.layer_indices
                .insert(untranslated.clone(), PcbLayerId(layer));
            self.layer_masks
                .insert(untranslated, Lset::from_layers(&[PcbLayerId(layer)]));
        }

        self.layer_masks
            .insert("*.Cu".into(), Lset::all_cu_mask().clone());
        self.layer_masks
            .insert("*In.Cu".into(), Lset::internal_cu_mask().clone());
        self.layer_masks.insert(
            "F&B.Cu".into(),
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]),
        );
        self.layer_masks.insert(
            "*.Adhes".into(),
            Lset::from_layers(&[PcbLayerId::BAdhes, PcbLayerId::FAdhes]),
        );
        self.layer_masks.insert(
            "*.Paste".into(),
            Lset::from_layers(&[PcbLayerId::BPaste, PcbLayerId::FPaste]),
        );
        self.layer_masks.insert(
            "*.Mask".into(),
            Lset::from_layers(&[PcbLayerId::BMask, PcbLayerId::FMask]),
        );
        self.layer_masks.insert(
            "*.SilkS".into(),
            Lset::from_layers(&[PcbLayerId::BSilkS, PcbLayerId::FSilkS]),
        );
        self.layer_masks.insert(
            "*.Fab".into(),
            Lset::from_layers(&[PcbLayerId::BFab, PcbLayerId::FFab]),
        );
        self.layer_masks.insert(
            "*.CrtYd".into(),
            Lset::from_layers(&[PcbLayerId::BCrtYd, PcbLayerId::FCrtYd]),
        );

        // This is for the first pretty & *.kicad_pcb formats, which had
        // Inner1_Cu - Inner14_Cu with the numbering sequence
        // reversed from the subsequent format's In1_Cu - In30_Cu numbering scheme.
        // The newer format brought in an additional 16 Cu layers and flipped the cu stack but
        // kept the gap between one of the outside layers and the last cu internal.
        for i in 1..=14 {
            let key = format!("Inner{i}.Cu");

            self.layer_masks.insert(
                key,
                Lset::from_layers(&[PcbLayerId(PcbLayerId::In15Cu.0 - 2 * i)]),
            );
        }
    }

    // ---- accessors -----------------------------------------------------------------------

    /// `IsTooRecent()`: whether a version number, if any was parsed, was too recent.
    pub fn is_too_recent(&self) -> bool {
        self.too_recent
    }

    /// `m_requiredVersion`: the file format version (0 before a `(version ...)` was read).
    pub fn required_version(&self) -> i32 {
        self.required_version
    }

    /// `GetParseWarnings()`.
    pub fn parse_warnings(&self) -> &[String] {
        &self.report.warnings
    }

    /// Push a non-fatal warning (`m_parseWarnings.push_back`).
    pub(crate) fn push_warning(&mut self, warning: impl Into<String>) {
        self.report.warnings.push(warning.into());
    }

    /// The parsed board and the report.
    pub fn into_board_and_report(self) -> (Board, ParseReport) {
        (self.board, self.report)
    }

    // ---- staging ---------------------------------------------------------------------------

    /// The body of a staging stub: called with the keyword of the current list as
    /// [`cur_tok`](kicad_sexpr::DsnLexer::cur_tok) (the opening `(` and the keyword consumed,
    /// as when the C++ calls `parseXXX`). Fails with "port pending: <token> (<package>)"
    /// unless `allow_pending`, in which case the rest of the list is skipped (`skipCurrent`)
    /// and recorded in the report.
    pub(crate) fn pending(&mut self, package: &'static str) -> Result<(), IoError> {
        let token = self.lexer.cur_text().to_string();

        if !self.options.allow_pending {
            return Err(IoError::new(format!("port pending: {token} ({package})")));
        }

        let line = self.lexer.cur_line_number();
        self.skip_current()?;
        self.report.skipped.push(SkippedItem {
            token,
            line,
            package,
        });
        Ok(())
    }

    // ---- token-level helpers ----------------------------------------------------------------

    /// Port of `skipCurrent`: skip the current token level, i.e. search for the RIGHT
    /// parenthesis which closes the current description.
    pub(crate) fn skip_current(&mut self) -> Result<(), IoError> {
        let mut curr_level = 0;

        loop {
            let token = self.lexer.next_tok()?;

            if token == T::T_EOF {
                return Ok(());
            }

            if token == T::T_LEFT {
                curr_level -= 1;
            }

            if token == T::T_RIGHT {
                curr_level += 1;

                if curr_level > 0 {
                    return Ok(());
                }
            }
        }
    }

    /// Port of `pushValueIntoMap`: add `value` in the net code mapping at `index`, growing the
    /// table if needed. A negative index is a programmer error in C++ (`resize` of a huge
    /// size); it is ignored here.
    pub(crate) fn push_value_into_map(&mut self, index: i32, value: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };

        if self.net_codes.len() <= index {
            self.net_codes.resize(index + 1, 0);
        }

        self.net_codes[index] = value;
    }

    /// Port of `getNetCode`: convert a legacy file net code using the mapping table if
    /// available, otherwise return it unchanged (negative or out of range).
    pub(crate) fn get_net_code(&self, net_code: i32) -> i32 {
        if net_code >= 0 && (net_code as usize) < self.net_codes.len() {
            return self.net_codes[net_code as usize];
        }

        net_code
    }

    /// Port of `parseBoardUnits()`: the current token in mm, converted to IU and clamped to
    /// `INT_LIMIT` before `KiROUND`.
    ///
    /// There should be no major rounding issues here, since the values in the file are in mm
    /// and get converted to nano-meters.
    pub(crate) fn parse_board_units(&self) -> Result<i32, IoError> {
        let retval = self.lexer.parse_double()? * PCB_IU_SCALE.iu_per_mm;

        // N.B. we currently represent board units as integers.  Any values that are
        // larger or smaller than those board units represent undefined behavior for
        // the system.  We limit values to the largest that is visible on the screen
        Ok(ki_round(clamp(retval, -INT_LIMIT, INT_LIMIT)))
    }

    /// Port of `parseBoardUnits( const char* aExpected, EDA_DATA_TYPE aDataType )`: read the
    /// next token (a number) and convert it with the scale of `data_type`.
    pub(crate) fn parse_board_units_expected(
        &mut self,
        expected: &str,
        data_type: EdaDataType,
    ) -> Result<i32, IoError> {
        let scale = kicad_core::eda_units::get_scale_for_internal_unit_type(
            &PCB_IU_SCALE,
            data_type,
        );
        let retval = self.lexer.parse_double_expected(expected)? * scale;

        Ok(ki_round(clamp(retval, -INT_LIMIT, INT_LIMIT)))
    }

    /// `parseBoardUnits( "..." )` with the default `EDA_DATA_TYPE::DISTANCE`.
    pub(crate) fn parse_board_units_str(&mut self, expected: &str) -> Result<i32, IoError> {
        self.parse_board_units_expected(expected, EdaDataType::Distance)
    }

    /// Port of the inline `parseBoardUnits( PCB_KEYS_T::T aToken, EDA_DATA_TYPE )`: the token
    /// text names the expected value.
    pub(crate) fn parse_board_units_tok(
        &mut self,
        token: T,
        data_type: EdaDataType,
    ) -> Result<i32, IoError> {
        let expected = self.lexer.token_text(token);
        self.parse_board_units_expected(expected, data_type)
    }

    /// `parseInt()`: `(int) strtol( CurText(), nullptr, 10 )`.
    pub(crate) fn parse_int(&self) -> i32 {
        self.lexer.parse_int()
    }

    /// `parseInt( aExpected )`: `NeedNUMBER( aExpected )` then `parseInt()`.
    pub(crate) fn parse_int_expected(&mut self, expected: &str) -> Result<i32, IoError> {
        self.lexer.parse_int_expected(expected)
    }

    /// `parseHex()`: `NextTok()` then `strtol( CurText(), nullptr, 16 )` (a C `long`).
    pub(crate) fn parse_hex(&mut self) -> Result<i64, IoError> {
        self.lexer.parse_hex()
    }

    /// `parseDouble()` of the current token.
    pub(crate) fn parse_double(&self) -> Result<f64, IoError> {
        self.lexer.parse_double()
    }

    /// `parseDouble( aExpected )`: the next token must be a number.
    pub(crate) fn parse_double_expected(&mut self, expected: &str) -> Result<f64, IoError> {
        self.lexer.parse_double_expected(expected)
    }

    /// Port of `parseBool`: `yes` or `no`.
    pub(crate) fn parse_bool(&mut self) -> Result<bool, IoError> {
        let token = self.lexer.next_tok()?;

        if token == T::T_yes {
            Ok(true)
        } else if token == T::T_no {
            Ok(false)
        } else {
            Err(self.lexer.expecting_str("yes or no"))
        }
    }

    /// Port of `parseOptBool`: `yes`, `no` or `none` (`None`).
    pub(crate) fn parse_opt_bool(&mut self) -> Result<Option<bool>, IoError> {
        let token = self.lexer.next_tok()?;

        if token == T::T_yes {
            Ok(Some(true))
        } else if token == T::T_no {
            Ok(Some(false))
        } else if token == T::T_none {
            Ok(None)
        } else {
            Err(self.lexer.expecting_str("yes, no or none"))
        }
    }

    /// Port of `parseMaybeAbsentBool( aDefaultValue )`: parses a boolean flag inside a list
    /// that existed before boolean normalization, e.g. `hide`, `hide)` and `(hide yes)`.
    ///
    /// Call it after the flag keyword. If the keyword followed a `(`, accepts `)` (=> the
    /// default), `yes`/`true` or `no`/`false` and consumes the closing `)`; for a bare keyword
    /// it returns the default without consuming anything.
    pub(crate) fn parse_maybe_absent_bool(&mut self, default_value: bool) -> Result<bool, IoError> {
        if self.lexer.prev_tok() != T::T_LEFT {
            // "hide"
            return Ok(default_value);
        }

        let token = self.lexer.next_tok()?;

        // "hide)"
        if token == T::T_RIGHT {
            return Ok(default_value);
        }

        let ret = if token == T::T_yes || token == T::T_true {
            true
        } else if token == T::T_no || token == T::T_false {
            false
        } else {
            return Err(self.lexer.expecting_str("yes or no"));
        };

        self.lexer.need_right()?;

        Ok(ret)
    }

    /// Port of `parseFrontBackOptBool( aAllowLegacyFormat )`: `((front B) (back B))` with `B`
    /// one of yes / no / none, or (legacy) a list of `front`, `back`, `none` keywords. Reads up
    /// to and including the closing `)` of the enclosing list.
    pub(crate) fn parse_front_back_opt_bool(
        &mut self,
        allow_legacy_format: bool,
    ) -> Result<(Option<bool>, Option<bool>), IoError> {
        let mut token = self.lexer.next_tok()?;

        let mut front: Option<bool> = None;
        let mut back: Option<bool> = None;

        if token != T::T_LEFT && allow_legacy_format {
            // legacy format for tenting.
            while token != T::T_RIGHT {
                if token == T::T_front {
                    front = Some(true);
                } else if token == T::T_back {
                    back = Some(true);
                } else if token == T::T_none {
                    front = None;
                    back = None;
                } else {
                    return Err(self.lexer.expecting_str("front, back or none"));
                }

                token = self.lexer.next_tok()?;
            }

            return Ok((front, back));
        }

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting_str("("));
            }

            token = self.lexer.next_tok()?;

            if token == T::T_front {
                front = self.parse_opt_bool()?;
            } else if token == T::T_back {
                back = self.parse_opt_bool()?;
            } else {
                return Err(self.lexer.expecting_str("front or back"));
            }

            self.lexer.need_right()?;

            token = self.lexer.next_tok()?;
        }

        Ok((front, back))
    }

    // ---- nets ----------------------------------------------------------------------------------

    /// Port of `parseNet( BOARD_CONNECTED_ITEM* aItem )` with the non-virtual
    /// `BOARD_CONNECTED_ITEM::SetNet`; see [`parse_net_with`](Self::parse_net_with).
    pub(crate) fn parse_net<I: BoardItem + ?Sized>(&mut self, item: &mut I) -> Result<(), IoError> {
        self.parse_net_with(item, |item, code| {
            if let Some(connected) = item.connected_mut() {
                connected.net = code;
            }
        })
    }

    /// Port of `parseNet( BOARD_CONNECTED_ITEM* aItem )`; call it after the `net` keyword.
    ///
    /// Legacy files (pre-10.0) have a net code instead of a net name. This net code is
    /// authoritative (though it may be mapped by `getNetCode()` to prevent collisions) and is
    /// applied with `SetNetCode( max( 0, code ), aNoAssert = true )` (0 off copper, unknown
    /// codes orphan the item). A name is looked up (`FindNet`) or a new net is created
    /// (`new NETINFO_ITEM( m_board, name )` + `Add( INSERT )`: `find_or_add_net( name, None )`)
    /// and handed to `set_net`, the item's (virtual: `ZONE` overrides it) `SetNet`. Without a
    /// board the name is ignored.
    ///
    /// `item` is the local value being parsed; its `base.parent` must already be set (C++
    /// constructs items with their parent), because `SetNetCode` asks `GetBoard()`.
    pub(crate) fn parse_net_with<I: BoardItem + ?Sized>(
        &mut self,
        item: &mut I,
        set_net: impl FnOnce(&mut I, NetCode),
    ) -> Result<(), IoError> {
        let token = self.lexer.next_tok()?;

        if token == T::T_NUMBER {
            let code = 0.max(self.get_net_code(self.parse_int()));
            // The C++ logs "Invalid net ID" to a trace channel when SetNetCode fails.
            self.set_net_code(item, NetCode(code));
            self.lexer.need_right()?;
            return Ok(());
        }

        if !kicad_sexpr::DsnLexer::<T>::is_symbol(token) {
            return Err(self.lexer.expecting_str("net name"));
        }

        if self.board_present {
            let mut net_name = self.lexer.from_utf8();

            // Convert overbar syntax from `~...~` to `~{...}`.  These were left out of the
            // first merge so the version is a bit later.
            if self.required_version < 20210606 {
                net_name = kicad_core::string_utils::convert_to_new_overbar_notation(&net_name);
            }

            let code = self.board.find_or_add_net(&net_name, None);
            set_net(item, code);
        }

        self.lexer.need_right()
    }

    /// Port of `BOARD_CONNECTED_ITEM::SetNetCode( aNetCode, aNoAssert = true )` on a local value
    /// whose parent chain is set: 0 off copper; the board's net of that code (orphaned, -1, if
    /// there is none); net 0 (`OrphanedItem()`) for a negative code or without a board. Returns
    /// `m_netinfo != nullptr`.
    pub(crate) fn set_net_code<I: BoardItem + ?Sized>(&self, item: &mut I, code: NetCode) -> bool {
        let mut code = code;

        {
            let ctx = crate::ctx::BoardCtx::bare(&self.board);

            if !item.is_on_copper_layer(&ctx) {
                code = NetCode::UNCONNECTED;
            }
        }

        // if aNetCode < 0 (typically NETINFO_LIST::FORCE_ORPHANED) or no parent board,
        // set the m_netinfo to the dummy NETINFO_LIST::ORPHANED
        let has_board = self.board_present && self.parent_reaches_board(item.base().parent);

        let new_net = if code.0 >= 0 && has_board {
            if self.board.find_net(code).is_some() {
                code
            } else {
                NetCode::ORPHANED
            }
        } else {
            NetCode::UNCONNECTED
        };

        if let Some(connected) = item.connected_mut() {
            connected.net = new_net;
        }

        new_net != NetCode::ORPHANED
    }

    /// `GetBoard() != nullptr` for an item with parent `parent` (the parent chain reaches the
    /// board).
    pub(crate) fn parent_reaches_board(&self, parent: ParentRef) -> bool {
        let mut parent = parent;

        // A parent chain is at most footprint -> table -> cell deep; bound the walk anyway.
        for _ in 0..16 {
            match parent {
                ParentRef::None => return false,
                ParentRef::Board => return true,
                ParentRef::Item(id) => match self.board.get(id) {
                    Some(item) => parent = item.base().parent,
                    None => return false,
                },
            }
        }

        false
    }

    // ---- version -------------------------------------------------------------------------------

    /// Port of `GetRequiredVersion`: the version of KiCad required to open this file, as
    /// `wxDateTime::FormatDate()` prints the date code in the reference build's C locale
    /// (`%x` = `MM/DD/YY`, checked against the oracle: 29990101 -> "01/01/99").
    ///
    /// # Errors
    ///
    /// `PARSE_ERROR` "Cannot interpret date code %d" when the version is not a valid date
    /// (wx would assert instead).
    pub fn get_required_version(&self) -> Result<String, IoError> {
        let v = self.required_version;
        let year = v / 10000;
        let month = (v / 100) - (year * 100);
        let day = v - (year * 10000) - (month * 100);

        // wx throws an assertion, not a catchable exception, when the date is invalid.
        // User input shouldn't give wx asserts, so check manually and throw a proper
        // error instead
        if day <= 0 || month <= 0 || month > 12 || day > days_in_month(month, year) {
            return Err(self
                .lexer
                .parse_error(format!("Cannot interpret date code {v}")));
        }

        // strftime %y: the year modulo 100, two digits.
        Ok(format!("{:02}/{:02}/{:02}", month, day, year.rem_euclid(100)))
    }

    // ---- geometry helpers ----------------------------------------------------------------------

    /// Port of `parseXY()`: a coordinate pair `(xy X Y)` in board units. Accepts being called
    /// with the `(` already read (`CurTok() == T_LEFT`) or not.
    pub(crate) fn parse_xy(&mut self) -> Result<Vector2I, IoError> {
        if self.lexer.cur_tok() != T::T_LEFT {
            self.lexer.need_left()?;
        }

        let token = self.lexer.next_tok()?;

        if token != T::T_xy {
            return Err(self.lexer.expecting(T::T_xy));
        }

        let x = self.parse_board_units_str("X coordinate")?;
        let y = self.parse_board_units_str("Y coordinate")?;

        self.lexer.need_right()?;

        Ok(Vector2I::new(x, y))
    }

    /// Port of `parseOutlinePoints( SHAPE_LINE_CHAIN& aPoly )`: one `(xy x y)` or
    /// `(arc (start ..) (mid ..) (end ..))` (sub-tokens in any order, all three required),
    /// appended to `poly`.
    pub(crate) fn parse_outline_points(&mut self, poly: &mut ShapeLineChain) -> Result<(), IoError> {
        if self.lexer.cur_tok() != T::T_LEFT {
            self.lexer.need_left()?;
        }

        let mut token = self.lexer.next_tok()?;

        match token {
            T::T_xy => {
                let x = self.parse_board_units_str("X coordinate")?;
                let y = self.parse_board_units_str("Y coordinate")?;

                self.lexer.need_right()?;

                poly.append_xy(x, y, false);
            }
            T::T_arc => {
                let mut has_start = false;
                let mut has_mid = false;
                let mut has_end = false;

                let mut arc_start = Vector2I::default();
                let mut arc_mid = Vector2I::default();
                let mut arc_end = Vector2I::default();

                token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    if token != T::T_LEFT {
                        return Err(self.lexer.expecting(T::T_LEFT));
                    }

                    token = self.lexer.next_tok()?;

                    match token {
                        T::T_start => {
                            arc_start.x = self.parse_board_units_str("start x")?;
                            arc_start.y = self.parse_board_units_str("start y")?;
                            has_start = true;
                        }
                        T::T_mid => {
                            arc_mid.x = self.parse_board_units_str("mid x")?;
                            arc_mid.y = self.parse_board_units_str("mid y")?;
                            has_mid = true;
                        }
                        T::T_end => {
                            arc_end.x = self.parse_board_units_str("end x")?;
                            arc_end.y = self.parse_board_units_str("end y")?;
                            has_end = true;
                        }
                        _ => return Err(self.lexer.expecting_str("start, mid or end")),
                    }

                    self.lexer.need_right()?;
                    token = self.lexer.next_tok()?;
                }

                if !has_start {
                    return Err(self.lexer.expecting_str("start"));
                }

                if !has_mid {
                    return Err(self.lexer.expecting_str("mid"));
                }

                if !has_end {
                    return Err(self.lexer.expecting_str("end"));
                }

                let arc = ShapeArc::from_start_mid_end(arc_start, arc_mid, arc_end, 0);

                poly.append_arc(&arc);

                if token != T::T_RIGHT {
                    return Err(self.lexer.expecting(T::T_RIGHT));
                }
            }
            _ => return Err(self.lexer.expecting_str("xy or arc")),
        }

        Ok(())
    }

    /// Port of `parseMargins`: `(left, top, right, bottom)` in board units.
    pub(crate) fn parse_margins(&mut self) -> Result<(i32, i32, i32, i32), IoError> {
        let left = self.parse_board_units_str("left margin")?;
        let top = self.parse_board_units_str("top margin")?;
        let right = self.parse_board_units_str("right margin")?;
        let bottom = self.parse_board_units_str("bottom margin")?;

        Ok((left, top, right, bottom))
    }

    // ---- properties ------------------------------------------------------------------------

    /// Port of `parseBoardProperty`: `"key" "value")` -> (key, value).
    pub(crate) fn parse_board_property(&mut self) -> Result<(String, String), IoError> {
        self.lexer.need_symbol()?;
        let name = self.lexer.from_utf8();
        self.lexer.need_symbol()?;
        let value = self.lexer.from_utf8();
        self.lexer.need_right()?;

        Ok((name, value))
    }

    /// Port of `parseCustomProperty( EDA_ITEM* aItem )`: `"key" "value")` stored with
    /// `EDA_ITEM::SetCustomProperty` (case-insensitive key match).
    pub(crate) fn parse_custom_property_item<I: BoardItem + ?Sized>(
        &mut self,
        item: &mut I,
    ) -> Result<(), IoError> {
        self.lexer.need_symbol()?;
        let key = self.lexer.from_utf8();
        self.lexer.need_symbol()?;
        let value = self.lexer.from_utf8();
        item.base_mut().set_custom_property(&key, &value);
        self.lexer.need_right()
    }

    /// Port of `parseCustomProperty( std::map<wxString, wxString>& aProps )`:
    /// `aProps[key] = value`.
    pub(crate) fn parse_custom_property_map(
        &mut self,
        props: &mut BTreeMap<String, String>,
    ) -> Result<(), IoError> {
        self.lexer.need_symbol()?;
        let key = self.lexer.from_utf8();
        self.lexer.need_symbol()?;
        let value = self.lexer.from_utf8();
        props.insert(key, value);
        self.lexer.need_right()
    }

    // ---- layers ------------------------------------------------------------------------------

    /// Port of `lookUpLayerSet( m_layerMasks )`: the set of the current token's name, or
    /// `{ Rescue }` for an unknown name (not recorded as undefined).
    pub(crate) fn look_up_layer_set(&self) -> Lset {
        match self.layer_masks.get(self.lexer.cur_text()) {
            Some(set) => set.clone(),
            None => Lset::from_layers(&[PcbLayerId::Rescue]),
        }
    }

    /// Port of `lookUpLayer( m_layerIndices, aAllowUndefined )`: the layer of the current
    /// token's name. An unknown name is recorded in `m_undefinedLayers` and gives `Rescue`
    /// (`"UNDEFINED"` gives `UNDEFINED_LAYER` when allowed).
    pub(crate) fn look_up_layer(&mut self, allow_undefined: bool) -> PcbLayerId {
        let cur_text = self.lexer.cur_text();

        match self.layer_indices.get(cur_text) {
            None => {
                if cur_text == "UNDEFINED" && allow_undefined {
                    return PcbLayerId::UndefinedLayer;
                }

                let name = cur_text.to_string();
                self.undefined_layers.insert(name);
                PcbLayerId::Rescue
            }
            Some(&layer) => {
                // Some files may have saved items to the Rescue Layer due to an issue in v5
                if layer == PcbLayerId::Rescue {
                    let name = cur_text.to_string();
                    self.undefined_layers.insert(name);
                }

                layer
            }
        }
    }

    /// Port of `parseBoardItemLayer`: call with `CurTok() == T_layer`; reads the layer name
    /// (the closing `)` is left to the caller).
    pub(crate) fn parse_board_item_layer(&mut self) -> Result<PcbLayerId, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_layer,
            "Cannot parse {} as layer.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_layer {
            return Ok(PcbLayerId::UndefinedLayer);
        }

        self.lexer.next_tok()?;

        Ok(self.look_up_layer(false))
    }

    /// Port of `parseBoardItemLayersAsMask`: call with `CurTok() == T_layers`; the union of the
    /// sets of all names up to and including the closing `)`.
    pub(crate) fn parse_board_item_layers_as_mask(&mut self) -> Result<Lset, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_layers,
            "Cannot parse {} as item layers.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_layers {
            return Ok(Lset::new());
        }

        let mut layer_mask = Lset::new();

        loop {
            let token = self.lexer.next_tok()?;

            if token == T::T_RIGHT {
                break;
            }

            layer_mask |= &self.look_up_layer_set();
        }

        Ok(layer_mask)
    }

    /// Port of `parseLayersForCuItemWithSoldermask`: the layers of an item with a single
    /// copper layer and an optional solder mask layer on the same (outer) side.
    pub(crate) fn parse_layers_for_cu_item_with_soldermask(&mut self) -> Result<Lset, IoError> {
        let layer_mask = self.parse_board_item_layers_as_mask()?;
        let masks = Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]);

        if (&layer_mask & Lset::all_cu_mask()).count() != 1 {
            return Err(self.lexer.expecting_str("single copper layer"));
        }

        if (&layer_mask & &masks).count() > 1 {
            return Err(self.lexer.expecting_str("max one soldermask layer"));
        }

        if (&layer_mask & Lset::internal_cu_mask()).any() && (&layer_mask & &masks).any() {
            return Err(self
                .lexer
                .expecting_str("no mask layer when track is on internal layer"));
        }

        if (&layer_mask & &Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BMask])).count() > 1 {
            return Err(self.lexer.expecting_str("copper and mask on the same side"));
        }

        if (&layer_mask & &Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::FMask])).count() > 1 {
            return Err(self.lexer.expecting_str("copper and mask on the same side"));
        }

        Ok(layer_mask)
    }

    // ---- UUIDs -------------------------------------------------------------------------------

    /// Port of `CurStrToKIID`: the current token as a `KIID` (legacy time stamps and malformed
    /// strings follow `KIID( std::string )`; a malformed string draws a random id from
    /// `board.kiid_generator`). In append mode (PCBIO-16) a fresh id would be recorded in
    /// `m_resetKIIDMap`.
    pub(crate) fn cur_str_to_kiid(&mut self) -> Kiid {
        let mut id_str: &str = self.lexer.cur_str();

        // Older files did not quote UUIDs. (The C++ keeps the trailing quote: substr( 1,
        // length - 1 ) runs to the end of the string.)
        if id_str.len() >= 2 && id_str.starts_with('"') && id_str.ends_with('"') {
            id_str = &id_str[1..];
        } else if id_str == "\"" {
            id_str = "";
        }

        let id_str = id_str.to_string();

        if self.append_to_existing {
            let id = Kiid::new(&mut self.board.kiid_generator);
            self.reset_kiid_map.entry(id_str).or_insert(id);
            id
        } else {
            Kiid::from_string_or_random(&id_str, &mut self.board.kiid_generator)
        }
    }
}

/// `std::clamp( v, lo, hi )` for doubles: `v < lo ? lo : hi < v ? hi : v` (NaN passes through).
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// `wxDateTime::GetNumberOfDays( month - 1, year )` (Gregorian calendar).
fn days_in_month(month: i32, year: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;

            if leap { 29 } else { 28 }
        }
    }
}

#[cfg(test)]
mod tests;
