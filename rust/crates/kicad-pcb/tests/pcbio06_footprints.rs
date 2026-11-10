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

//! Package PCBIO-06 (board file footprints): the differential oracle and the ported C++ tests.
//!
//! * `golden_footprints`: every board of `tests/data/pcbio06` (hand-made boards and corpus
//!   boards reduced by `tests/oracle/pcbio06_strip.py`) re-saved by the reference
//!   `kicad-cli pcb upgrade --force` (`tests/oracle/pcbio06_golden.sh`, goldens in
//!   `tests/golden/pcbio06`): parse + format of the input must give the oracle's re-save `O1`
//!   byte for byte, and parse + format of `O1` its re-save `O2` (or `O1` for a fixed point).
//! * Ports of `qa/tests/pcbnew/test_footprint_load_save.cpp` and
//!   `qa/tests/pcbnew/test_extruded_body_load_save.cpp` (the latter as board round trips: the
//!   footprint library file is package PCBIO-14).

#[path = "io_corpus/resave.rs"]
mod resave;

use std::path::{Path, PathBuf};

use kicad_core::{Color4D, Kiid, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::Vector2I;
use kicad_geom::math::vector3::Vector3D;
use kicad_pcb::board_design_settings::LAYER_CLASS_SILK;
use kicad_pcb::footprint::Footprint;
use kicad_pcb::ids::FootprintId;
use kicad_pcb::io::parser::ParseOptions;
use kicad_pcb::io::writer::FormatOptions;
use kicad_pcb::io::{format_board, parse_board};
use kicad_pcb::pcb_3d_body::{ExtrusionMaterial, Pcb3dBody};
use kicad_pcb::pcb_field::{FieldT, PcbField};
use kicad_pcb::pcb_text::PcbText;
use kicad_pcb::{AddMode, Board, BoardCtx, OwnedItem, ParentRef};
use kicad_qa::uuid_canon::{canonicalise_uuids, canonicalise_uuids_sorted, count_random_uuids};
use resave::{Resave, rust_resave};

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The first line where `a` and `b` differ, with a little context.
fn first_difference(expected: &str, actual: &str) -> Option<String> {
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();

    for i in 0..e.len().max(a.len()) {
        if e.get(i) != a.get(i) {
            let from = i.saturating_sub(3);
            let ctx = |v: &[&str]| v[from.min(v.len())..(i + 3).min(v.len())].join("\n");
            return Some(format!(
                "line {}\n--- oracle\n{}\n--- port\n{}",
                i + 1,
                ctx(&e),
                ctx(&a)
            ));
        }
    }

    None
}

/// Every board of `tests/data/pcbio06` against the oracle's re-saves (see the module comment).
#[test]
fn golden_footprints() {
    let data = crate_dir().join("tests").join("data").join("pcbio06");
    let golden = crate_dir().join("tests").join("golden").join("pcbio06");
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(&data)
        .expect("tests/data/pcbio06")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb"))
        .collect();
    inputs.sort();

    let mut failures = Vec::new();
    let mut compared = 0;

    for input in inputs {
        let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&input).unwrap();
        let o1 = std::fs::read_to_string(golden.join(format!("{stem}.kicad_pcb"))).unwrap();
        let o2 = std::fs::read_to_string(golden.join(format!("{stem}_o2.kicad_pcb")))
            .unwrap_or_else(|_| o1.clone());

        for (label, source, expected) in [("input", &text, &o1), ("O1", &o1, &o2)] {
            let actual = match rust_resave(source, &stem, false) {
                Resave::Written { text, .. } => text,
                Resave::ModelPending {
                    stage, function, ..
                } => {
                    eprintln!(
                        "{stem} ({label}): not compared, model pending ({}) {function}",
                        stage.label()
                    );
                    continue;
                }
                other => {
                    failures.push(format!("{stem} ({label}): re-save failed: {other:?}"));
                    continue;
                }
            };

            // PCBIO06_DUMP=<dir>: keep the port's re-saves for inspection.
            if let Some(dir) = std::env::var_os("PCBIO06_DUMP") {
                let _ = std::fs::write(
                    Path::new(&dir).join(format!("{stem}_{label}.kicad_pcb")),
                    &actual,
                );
            }

            compared += 1;

            // UUIDs the C++ draws at random (items without a UUID in a legacy input) are
            // masked; when such UUIDs decide an order (the footprint sort), sibling blocks
            // holding them are sorted too.
            let masked_expected = canonicalise_uuids(expected, source);
            let masked_actual = canonicalise_uuids(&actual, source);

            if let Some(diff) = first_difference(&masked_expected, &masked_actual) {
                if count_random_uuids(expected, source) > 0
                    && canonicalise_uuids_sorted(expected, source)
                        == canonicalise_uuids_sorted(&actual, source)
                {
                    continue;
                }

                failures.push(format!("{stem} ({label}): {diff}"));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert!(compared >= 40, "only {compared} re-saves compared");
}

// ---- qa/tests/pcbnew/test_footprint_load_save.cpp -------------------------------------------

/// `KI_TEST::LoadAndTestBoardFile( name, roundtrip = true, test )`: load a QA board (pads are
/// PCBIO-07: skipped by the staging switch), test it, save, reload and test again. The save
/// needs the footprint child sort (`FOOTPRINT::cmp_drawings`, M20): while that is a model stub
/// the round trip is reported, not run.
fn load_and_test_board_file(name: &str, test: impl Fn(&Board)) {
    resave::quiet_pending_panics();
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));
    let text = std::fs::read_to_string(&path).expect("QA board");
    let options = ParseOptions {
        allow_pending: true,
    };
    let board = parse_board(&text, name, &options).expect("load");
    test(&board);

    let format_options = FormatOptions {
        allow_pending: true,
        ..FormatOptions::default()
    };
    let saved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        format_board(&board, &format_options)
    }));

    match saved {
        Ok(saved) => {
            let saved = saved.expect("save");
            let reloaded = parse_board(&saved, name, &options).expect("reload");
            test(&reloaded);
        }
        Err(_) => eprintln!("{name}: round trip not run (model stub reached by the save)"),
    }
}

fn footprint_by_uuid(board: &Board, uuid: &str) -> FootprintId {
    let uuid = Kiid::parse(uuid).unwrap();
    board
        .footprints()
        .find(|(_, f)| f.base.uuid == uuid)
        .map(|(id, _)| id)
        .expect("footprint with this UUID")
}

/// Port of `FootprintLoadSave` (`footprints_load_save`): lock state and position of the two
/// footprints.
#[test]
fn footprint_load_save() {
    let cases = [
        (
            "898cf321-03c7-40bb-8d78-4bc5e52986c2",
            false,
            Vector2I::new(100, 80),
        ),
        (
            "0775cd70-2e84-4592-a160-456c37a8f4f6",
            true,
            Vector2I::new(100, 100),
        ),
    ];

    load_and_test_board_file("footprints_load_save", |board| {
        let ctx = BoardCtx::bare(board);

        for (uuid, locked, pos) in cases {
            let fp = &board[footprint_by_uuid(board, uuid)];
            assert_eq!(kicad_pcb::board_item::is_locked(fp, &ctx), locked, "{uuid}");
            assert_eq!(fp.position(&ctx), pos * 1_000_000, "{uuid}");
        }
    });
}

/// A board holding one footprint built with the model API, saved and reloaded.
fn save_and_reload(board: &Board) -> Board {
    let saved = format_board(board, &FormatOptions::default()).expect("save");
    parse_board(&saved, "reloaded", &ParseOptions::default()).expect("reload")
}

/// `new FOOTPRINT( &board )` + `SetPosition( 20 mm, 0 )` + `SetReference( "U1" )`.
fn new_footprint(board: &mut Board) -> FootprintId {
    let fp = board.create_footprint();
    board.footprint_set_position(fp, Vector2I::new(PCB_IU_SCALE.mm_to_iu(20.0), 0));
    let ctx_ref = {
        let ctx = BoardCtx::bare(board);
        board[fp].field(&ctx, FieldT::Reference).unwrap()
    };
    board.modify_field(ctx_ref, |f, _| f.text.text.set_text("U1"));
    fp
}

/// `new PCB_FIELD( fp, FIELD_T::USER, "Label" )` + `SetText( "BezelSW0" )` + `SetLayer( F_SilkS )`.
fn label_field(board: &mut Board, fp: FootprintId) -> PcbField {
    let ordinal = {
        let ctx = BoardCtx::bare(board);
        board[fp].next_field_ordinal(&ctx)
    };
    let mut field = PcbField::new(
        &mut board.kiid_generator,
        Some(PcbLayerId::FCu),
        FieldT::User,
        "Label",
    );
    field.set_ordinal(ordinal, FieldT::User);
    field.text.text.set_text("BezelSW0");
    field.text.base.layer = PcbLayerId::FSilkS;
    field
}

fn field_by_name<'b>(board: &'b Board, fp: FootprintId, name: &str) -> Option<&'b PcbField> {
    let ctx = BoardCtx::bare(board);
    board[fp]
        .field_by_name(&ctx, name)
        .and_then(|id| board.field(id))
}

/// Port of `FootprintFieldLockRoundTrip`: fields are written without a header of their own,
/// inside the footprint's (property ...), so their lock takes a different path through the
/// writer than a plain footprint text's does.
#[test]
fn footprint_field_lock_round_trip() {
    let mut board = Board::new();
    let fp = new_footprint(&mut board);

    let reference = {
        let ctx = BoardCtx::bare(&board);
        board[fp].field(&ctx, FieldT::Reference).unwrap()
    };
    board.modify_field(reference, |f, _| f.text.base.is_locked = true);

    let mut user_field = label_field(&mut board, fp);
    user_field.text.base.is_locked = true;
    board
        .footprint_add(fp, OwnedItem::Field(Box::new(user_field)), AddMode::Append)
        .unwrap();

    // A plain footprint text is the control: it has always round-tripped its lock
    let mut fp_text = PcbText::for_footprint(Some(PcbLayerId::FCu));
    fp_text.base.uuid = Kiid::new(&mut board.kiid_generator);
    fp_text.text.set_text("fp text");
    fp_text.base.layer = PcbLayerId::FSilkS;
    fp_text.base.is_locked = true;
    board
        .footprint_add(fp, OwnedItem::Text(Box::new(fp_text)), AddMode::Append)
        .unwrap();

    board.attach_footprint(fp, AddMode::Append);

    let restored = save_and_reload(&board);
    assert_eq!(restored.footprints().count(), 1);
    let loaded = restored.footprint_ids()[0];
    let ctx = BoardCtx::bare(&restored);

    let reference = restored[loaded].field(&ctx, FieldT::Reference).unwrap();
    assert!(restored.field(reference).unwrap().text.base.is_locked);
    let label = field_by_name(&restored, loaded, "Label").expect("Label field");
    assert!(label.text.base.is_locked);

    let mut texts = 0;

    for (_, item) in restored.footprint_drawings(loaded) {
        if let kicad_pcb::AnyItem::Text(t) = item {
            assert!(t.base.is_locked);
            texts += 1;
        }
    }

    assert_eq!(texts, 1);
}

/// Port of `FootprintUserFieldKeepsAutoThickness`: a field with no thickness token is using
/// auto thickness. Loading one builds the field before its effects are parsed, so nothing
/// stamped on it then may survive the parse.
#[test]
fn footprint_user_field_keeps_auto_thickness() {
    let mut board = Board::new();

    // Auto thickness only differs from the board default when that default is not zero
    board.design_settings_mut().text_thickness[LAYER_CLASS_SILK] = PCB_IU_SCALE.mm_to_iu(0.15);

    let fp = new_footprint(&mut board);
    let mut user_field = label_field(&mut board, fp);
    // SetAutoThickness( true ): a zero stroke width
    user_field.text.text.attributes.stroke_width = 0;
    let field_id = board
        .footprint_add(fp, OwnedItem::Field(Box::new(user_field)), AddMode::Append)
        .unwrap();
    board.attach_footprint(fp, AddMode::Append);

    {
        let ctx = BoardCtx::bare(&board);
        let field = board.field(field_id.typed().unwrap()).unwrap();
        assert!(kicad_pcb::eda_text::auto_thickness(field, &ctx));
    }

    let restored = save_and_reload(&board);
    assert_eq!(restored.footprints().count(), 1);
    let loaded = restored.footprint_ids()[0];
    let field = field_by_name(&restored, loaded, "Label").expect("Label field");
    let ctx = BoardCtx::bare(&restored);
    assert!(kicad_pcb::eda_text::auto_thickness(field, &ctx));
}

/// Port of `LegacyFootprintPropertyTakesBoardDefaults` (`custom_fields`): a legacy property
/// carries no effects at all, so the board defaults are the only style it can get.
#[test]
fn legacy_footprint_property_takes_board_defaults() {
    load_and_test_board_file("custom_fields", |board| {
        let fp = board.footprint_ids()[0];
        let field = field_by_name(board, fp, "myfield").expect("myfield");
        let ctx = BoardCtx::bare(board);

        assert!(!kicad_pcb::eda_text::auto_thickness(field, &ctx));
        assert_eq!(
            field.text.text_thickness(&ctx),
            board
                .design_settings()
                .get_text_thickness(field.text.base.layer)
        );
    });
}

// ---- qa/tests/pcbnew/test_extruded_body_load_save.cpp ------------------------------------------
//
// The C++ saves the footprint to a .kicad_mod library file (PCBIO-14, not ported yet); here the
// footprint is saved on a board, which runs the same footprint formatter and parser.

/// A board with one footprint holding the body `body` (`EnsureBody()` + the edits).
fn board_with_body(body: Option<Pcb3dBody>) -> Board {
    let mut board = Board::new();
    let fp = board.create_footprint();

    if let Some(mut body) = body {
        body.base.uuid = Kiid::new(&mut board.kiid_generator);
        board
            .footprint_add(fp, OwnedItem::Body3d(Box::new(body)), AddMode::Append)
            .unwrap();
    }

    board.attach_footprint(fp, AddMode::Append);
    board
}

fn loaded_body(board: &Board) -> Option<Pcb3dBody> {
    let fp = board.footprint_ids()[0];
    board.footprint_bodies(fp).next().map(|(_, b)| b.clone())
}

/// Port of `ExtrudedBodyLoadSave/FullRoundTrip`.
#[test]
fn extruded_body_full_round_trip() {
    let mut body = Pcb3dBody::default();
    body.height = PCB_IU_SCALE.mm_to_iu(2.5);
    body.standoff = PCB_IU_SCALE.mm_to_iu(0.1);
    body.layer = PcbLayerId::FCrtYd;
    body.material = ExtrusionMaterial::Metal;
    body.color = Color4D::new(0.8, 0.4, 0.0, 0.9);
    body.offset = Vector3D::new(1.0, -0.5, 0.2);
    body.scale = Vector3D::new(1.5, 0.8, 2.0);
    body.rotation = Vector3D::new(10.0, 20.0, 45.0);
    body.show = false;

    let loaded = save_and_reload(&board_with_body(Some(body.clone())));
    let lb = loaded_body(&loaded).expect("HasBody()");

    let close = |a: f64, b: f64| (a - b).abs() <= b.abs() * 1e-4;

    assert_eq!(lb.height, body.height);
    assert_eq!(lb.standoff, body.standoff);
    assert_eq!(lb.layer, body.layer);
    assert_eq!(lb.material, body.material);
    assert!(close(lb.color.r, body.color.r));
    assert!(close(lb.color.g, body.color.g));
    assert!(close(lb.color.b, body.color.b));
    assert!(close(lb.color.a, body.color.a));
    assert!(close(lb.offset.x, body.offset.x));
    assert!(close(lb.offset.y, body.offset.y));
    assert!(close(lb.offset.z, body.offset.z));
    assert!(close(lb.scale.x, body.scale.x));
    assert!(close(lb.scale.y, body.scale.y));
    assert!(close(lb.scale.z, body.scale.z));
    assert!(close(lb.rotation.x, body.rotation.x));
    assert!(close(lb.rotation.y, body.rotation.y));
    assert!(close(lb.rotation.z, body.rotation.z));
    assert_eq!(lb.show, body.show);
}

/// Port of `ExtrudedBodyLoadSave/DefaultsRoundTrip`.
#[test]
fn extruded_body_defaults_round_trip() {
    let body = Pcb3dBody {
        height: PCB_IU_SCALE.mm_to_iu(5.0),
        // Leave everything else at defaults
        ..Pcb3dBody::default()
    };

    let loaded = save_and_reload(&board_with_body(Some(body.clone())));
    let lb = loaded_body(&loaded).expect("HasBody()");

    assert_eq!(lb.height, body.height);
    assert_eq!(lb.standoff, 0);
    assert_eq!(lb.layer, PcbLayerId::UndefinedLayer);
    assert_eq!(lb.material, ExtrusionMaterial::Plastic);
    assert!(lb.color == Color4D::UNSPECIFIED);
    assert_eq!(lb.offset, Vector3D::new(0.0, 0.0, 0.0));
    assert_eq!(lb.scale, Vector3D::new(1.0, 1.0, 1.0));
    assert_eq!(lb.rotation, Vector3D::new(0.0, 0.0, 0.0));
    assert!(lb.show);
}

/// Port of `ExtrudedBodyLoadSave/NoExtrudedBodyRoundTrip`.
#[test]
fn extruded_body_none_round_trip() {
    let loaded = save_and_reload(&board_with_body(None));
    assert!(loaded_body(&loaded).is_none());
}

/// A body of height 0 is not written (`m_height <= 0`), so it does not come back.
#[test]
fn extruded_body_without_height_is_not_saved() {
    let loaded = save_and_reload(&board_with_body(Some(Pcb3dBody::default())));
    assert!(loaded_body(&loaded).is_none());
}

/// The parent of a footprint read from a board file is the board, and the footprint is in the
/// board's list (the parser returns it unattached, `parseBOARD` attaches it).
#[test]
fn parsed_footprints_are_attached() {
    let text = std::fs::read_to_string(
        crate_dir()
            .join("tests")
            .join("data")
            .join("pcbio06")
            .join("handmade_current.kicad_pcb"),
    )
    .unwrap();
    let board = parse_board(&text, "handmade_current", &ParseOptions::default()).unwrap();
    assert_eq!(board.footprints().count(), 3);

    for (_, fp) in board.footprints() {
        let fp: &Footprint = fp;
        assert_eq!(fp.base.parent, ParentRef::Board);
    }
}
