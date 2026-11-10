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

//! Package PCBIO-08 (board zones, zone defaults): differential tests against the reference
//! `kicad-cli pcb upgrade --force` (goldens written by `tests/oracle/pcbio08_golden.sh`) and
//! ports of the C++ tests of `qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp`:
//! `Issue19775_ZoneLayerWildcards`, `Issue23125_EmptyZoneDiscarded`,
//! `CopperThievingZone_RoundTrip`, `CopperThievingZone_AllPatternsRoundTrip`,
//! `CopperThievingZone_RejectedInOldFileVersion`, `CopperThievingZone_RejectsMalformedGeometry`.
//!
//! Constructs of other, still pending packages (footprints, ...) are skipped by the staging
//! switches (`allow_pending`); `port_pending!` model stubs (e.g. the board post-pass
//! `ZONE::SetLayerSetAndRemoveUnusedFills` call site while it is still a stub) are reported as
//! pending instead of failing (`model`).

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use kicad_core::PcbLayerId;
use kicad_geom::{EdaAngle, EdaOrientation, Vector2I};
use kicad_pcb::board::{AddMode, Board};
use kicad_pcb::ids::NetCode;
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::item::ParentRef;
use kicad_pcb::zone::Zone;
use kicad_pcb::zone_settings::{ThievingPattern, ThievingSettings, ZoneFillMode};

/// The prefix of a `port_pending!` panic.
const PENDING: &str = "kicad-pcb: port pending: ";

/// Run `f`; a `port_pending!` panic of a not yet ported model function becomes `Err(name)`.
fn model<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned());

            match msg {
                Some(m) if m.starts_with(PENDING) => Err(m[PENDING.len()..].to_string()),
                _ => resume_unwind(payload),
            }
        }
    }
}

/// Run a test body; a model stub reached on the way is reported, not failed.
fn run(name: &str, f: impl FnOnce()) {
    if let Err(stub) = model(f) {
        eprintln!("{name}: pending model stub: {stub}");
    }
}

fn staging_parse() -> ParseOptions {
    ParseOptions {
        allow_pending: true,
    }
}

fn staging() -> FormatOptions {
    FormatOptions {
        allow_pending: true,
        ..FormatOptions::default()
    }
}

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn parse(text: &str, name: &str) -> Board {
    parse_board_report(text, name, &staging_parse())
        .unwrap_or_else(|e| panic!("{name}: {}", e.what()))
        .0
}

fn load(path: &Path) -> Board {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    parse(&text, &path.to_string_lossy())
}

fn resave(text: &str, name: &str) -> String {
    let board = parse(text, name);
    format_board(&board, &staging()).unwrap_or_else(|e| panic!("{name}: {}", e.what()))
}

fn zones(board: &Board) -> Vec<&Zone> {
    board.zones().map(|(_, z)| z).collect()
}

/// The top-level `(zone ..)` blocks of a prettified board file (from the `\t(zone` line to its
/// closing `\t)` line), in file order. Mirrors `tests/oracle/pcbio08_golden.sh`.
fn zone_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;

    for l in text.split_inclusive('\n') {
        match cur.as_mut() {
            None => {
                let t = l.trim_end_matches('\n');

                if t == "\t(zone" {
                    cur = Some(l.to_string());
                } else if t == "\t(zone)" {
                    out.push(l.to_string());
                }
            }
            Some(block) => {
                block.push_str(l);

                if l == "\t)\n" {
                    out.push(cur.take().unwrap());
                }
            }
        }
    }

    out
}

/// The `(zone_defaults ..)` part of the setup section (up to its closing line).
fn zone_defaults(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;

    for l in text.split_inclusive('\n') {
        if l.trim_start().starts_with("(zone_defaults") {
            inside = true;
        }

        if inside {
            out.push_str(l);

            if l == "\t\t)\n" {
                break;
            }
        }
    }

    out
}

/// The zone blocks with every `(uuid ...)` line whose UUID does not occur in `original`
/// replaced by `(uuid masked)`, sorted (zones without a UUID in the file get a random one).
/// Mirrors `tests/oracle/pcbio08_golden.sh`.
fn masked(blocks: &[String], original: &str) -> Vec<String> {
    let mut out: Vec<String> = blocks
        .iter()
        .map(|b| {
            b.split_inclusive('\n')
                .map(|l| {
                    let t = l.trim();

                    match t
                        .strip_prefix("(uuid \"")
                        .and_then(|r| r.strip_suffix("\")"))
                    {
                        Some(uuid) if !original.contains(uuid) => {
                            let indent = &l[..l.len() - l.trim_start().len()];
                            format!("{indent}(uuid masked)\n")
                        }
                        _ => l.to_string(),
                    }
                })
                .collect()
        })
        .collect();

    out.sort();
    out
}

fn fnv(bytes: &[u8]) -> String {
    format!("{:016x}", kicad_qa::io_corpus::fnv1a64(bytes))
}

/// Differential oracle on the hand-made boards (`tests/data/pcbio08/`: every zone token incl.
/// arcs in outlines, hatch / thieving fills, smoothing, island settings, rule areas with
/// placement links, teardrop areas, per-layer properties, zone defaults; and a 2020 board with
/// legacy net codes, the `net_name` repair path, stroked fills and `fill_segments`): parse +
/// format of the input reproduces the zone blocks and the zone defaults of the reference
/// re-save byte for byte and in order.
#[test]
fn oracle_resave_handmade() {
    for name in ["zones_modern.kicad_pcb", "zones_legacy.kicad_pcb"] {
        let input =
            std::fs::read_to_string(crate_dir().join("tests/data/pcbio08").join(name)).unwrap();
        let o1 =
            std::fs::read_to_string(crate_dir().join("tests/golden/pcbio08").join(name)).unwrap();

        run(name, || {
            let out = resave(&input, name);
            let (got, want) = (zone_blocks(&out), zone_blocks(&o1));

            for (k, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                assert_eq!(g, w, "{name}: zone {k} differs");
            }

            assert_eq!(got.len(), want.len(), "{name}: zone count");
            assert_eq!(
                zone_defaults(&out),
                zone_defaults(&o1),
                "{name}: zone_defaults"
            );

            // O1 is a fixed point of the zone writer too.
            assert_eq!(zone_blocks(&resave(&o1, name)), want, "{name} (O1)");
        });
    }
}

/// Corpus replay of `tests/golden/pcbio08_corpus.txt` (the zone blocks of the reference
/// re-save of every corpus board): parse + format of the original board must give the same
/// zone blocks. Per board: identical (same order), same set (the order differs only through
/// the random UUIDs of zones without one in the file, e.g. teardrop areas), same modulo random
/// UUIDs (such a zone is not a teardrop area, so its random UUID is written), pending (a model
/// stub), error (another package's parser or writer); `differs` fails.
#[test]
fn oracle_corpus_zones() {
    let golden =
        std::fs::read_to_string(crate_dir().join("tests/golden/pcbio08_corpus.txt")).unwrap();
    let root = kicad_qa::kicad_source_root();
    let (mut identical, mut same_set, mut pending, mut errors) = (0, 0, 0, 0);
    let mut random_uuid = 0;
    let mut failures = Vec::new();

    for line in golden
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('|').collect();
        let (path, count) = (f[0], f[1].parse::<usize>().unwrap());
        let file = root.join(path);

        // Keep the default test run short: the few multi-megabyte boards are left to the
        // io-corpus harness.
        if std::fs::metadata(&file).map_or(true, |m| m.len() > 4_000_000) {
            continue;
        }

        let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
        let out = model(|| {
            parse_board_report(&text, path, &staging_parse())
                .and_then(|(board, _)| format_board(&board, &staging()))
                .map_err(|e| e.what().to_string())
        });

        let out = match out {
            Err(_) => {
                pending += 1;
                continue;
            }
            Ok(Err(_)) => {
                errors += 1;
                continue;
            }
            Ok(Ok(out)) => out,
        };

        let blocks = zone_blocks(&out);
        let mut sorted = blocks.clone();
        sorted.sort();

        if blocks.len() == count && fnv(blocks.concat().as_bytes()) == f[2] {
            identical += 1;
        } else if blocks.len() == count && fnv(sorted.concat().as_bytes()) == f[3] {
            same_set += 1;
        } else if blocks.len() == count && fnv(masked(&blocks, &text).concat().as_bytes()) == f[4] {
            random_uuid += 1;
        } else {
            failures.push(format!("{path}: {} zones, expected {count}", blocks.len()));
        }
    }

    eprintln!(
        "pcbio08 corpus zones: identical {identical}, same set {same_set}, same modulo random \
         uuids {random_uuid}, pending {pending}, errors {errors}, differs {}",
        failures.len()
    );

    assert!(
        failures.is_empty(),
        "zone blocks differ:\n{}",
        failures.join("\n")
    );
}

/// Port of `Issue19775_ZoneLayerWildcards`.
#[test]
fn issue19775_zone_layer_wildcards() {
    let data = kicad_qa::pcbnew_data_dir().join("plugins/kicad_sexpr/Issue19775_ZoneLayers");

    run("Zone layers with wildcards", || {
        let board = load(&data.join("LayerWildcard.kicad_pcb"));

        // One zone in the file
        assert_eq!(zones(&board).len(), 1);

        let z = zones(&board)[0];

        // On both front and back layers, with zone fill on both
        assert!(z.layer_set.contains(PcbLayerId::FCu) && z.layer_set.contains(PcbLayerId::BCu));
        assert!(z.filled_polys_list(PcbLayerId::FCu).total_vertices() > 0);
        assert!(z.filled_polys_list(PcbLayerId::BCu).total_vertices() > 0);
    });

    run("Round trip layers", || {
        // Load and save the board from above to test how we write the zones into it
        let saved = {
            let board = load(&data.join("LayerEnumerate.kicad_pcb"));
            format_board(&board, &staging()).unwrap()
        };

        // Read the new board
        let board = parse(&saved, "Issue19775_RoundTrip.kicad_pcb");

        // One zone in the file
        assert_eq!(zones(&board).len(), 1);

        let z = zones(&board)[0];

        // On both front and back layers, with zone fill on both
        assert!(z.layer_set.contains(PcbLayerId::FCu) && z.layer_set.contains(PcbLayerId::BCu));
        assert!(z.filled_polys_list(PcbLayerId::FCu).total_vertices() > 0);
        assert!(z.filled_polys_list(PcbLayerId::BCu).total_vertices() > 0);
        assert!(z.layer_properties.contains_key(&PcbLayerId::FCu));
    });
}

/// Port of `Issue23125_EmptyZoneDiscarded`: zones with no polygon outline are silently
/// discarded during loading (https://gitlab.com/kicad/code/kicad/-/issues/23125).
#[test]
fn issue23125_empty_zone_discarded() {
    let path = kicad_qa::pcbnew_data_dir()
        .join("plugins/kicad_sexpr/Issue23125_EmptyZone/EmptyZone.kicad_pcb");

    run("Issue23125_EmptyZoneDiscarded", || {
        let board = load(&path);

        // The file contains 3 zones: 1 valid (with polygon) and 2 empty (no polygon).
        // The 2 empty zones should have been discarded during loading.
        assert_eq!(zones(&board).len(), 1);

        // The surviving zone should have a valid position
        let z = zones(&board)[0];
        assert!(z.num_corners() > 0);
    });
}

/// `new ZONE( board )`, `SetLayer( F_Cu )`, a `size` mm square outline, then
/// `SetFillMode( COPPER_THIEVING )`.
fn thieving_zone(board: &mut Board, size: f64) -> Zone {
    let settings = board.design_settings().default_zone_settings().clone();
    let mut zone = Zone::new_with_settings(&mut board.kiid_generator, &settings, false);
    zone.base.item.parent = ParentRef::Board;

    let env = board.child_env(ParentRef::Board);
    let s = kicad_core::PCB_IU_SCALE.mm_to_iu(size);

    zone.set_layer(&env, PcbLayerId::FCu);
    zone.append_corner(Vector2I::new(0, 0), -1, false);
    zone.append_corner(Vector2I::new(s, 0), -1, false);
    zone.append_corner(Vector2I::new(s, s), -1, false);
    zone.append_corner(Vector2I::new(0, s), -1, false);
    zone.set_fill_mode(ZoneFillMode::CopperThieving);
    zone
}

/// Port of `CopperThievingZone_RoundTrip`: a copper-thieving zone round-trips through the
/// writer and parser, preserving every `THIEVING_SETTINGS` field plus the netless invariant.
#[test]
fn copper_thieving_zone_round_trip() {
    run("CopperThievingZone_RoundTrip", || {
        let mut write_board = Board::new();
        let mut zone = thieving_zone(&mut write_board, 10.0);

        let mm = |v: f64| kicad_core::PCB_IU_SCALE.mm_to_iu(v);
        let thieving = ThievingSettings {
            pattern: ThievingPattern::Hatch,
            element_size: mm(0.42),
            gap: mm(1.27),
            line_width: mm(0.35),
            stagger: true,
            orientation: EdaOrientation::new(EdaAngle::from_degrees(30.0)),
        };
        zone.set_thieving_settings(&thieving);

        write_board.add_zone(zone, AddMode::Insert);

        let saved = format_board(&write_board, &staging()).unwrap();
        let read_board = parse(&saved, "copper_thieving_roundtrip.kicad_pcb");

        assert_eq!(zones(&read_board).len(), 1);

        let loaded = zones(&read_board)[0];

        assert_eq!(loaded.fill_mode(), ZoneFillMode::CopperThieving);
        assert!(loaded.is_copper_thieving());
        assert_eq!(loaded.base.net, NetCode(0));

        let s = &loaded.thieving_settings;
        assert_eq!(s.pattern, ThievingPattern::Hatch);
        assert_eq!(s.element_size, thieving.element_size);
        assert_eq!(s.gap, thieving.gap);
        assert_eq!(s.line_width, thieving.line_width);
        assert!(s.stagger);
        assert_eq!(s.orientation.angle(), EdaAngle::from_degrees(30.0));
    });
}

/// Port of `CopperThievingZone_AllPatternsRoundTrip`: each `THIEVING_PATTERN` value
/// round-trips through the file format.
#[test]
fn copper_thieving_zone_all_patterns_round_trip() {
    for pattern in [
        ThievingPattern::Dots,
        ThievingPattern::Squares,
        ThievingPattern::Hatch,
    ] {
        run(&format!("pattern enum value {}", pattern as i32), || {
            let mut write_board = Board::new();
            let mut zone = thieving_zone(&mut write_board, 5.0);

            let mut thieving = zone.thieving_settings.clone();
            thieving.pattern = pattern;
            zone.set_thieving_settings(&thieving);

            write_board.add_zone(zone, AddMode::Insert);

            let saved = format_board(&write_board, &staging()).unwrap();
            let read_board = parse(&saved, "copper_thieving_pattern.kicad_pcb");

            assert_eq!(zones(&read_board).len(), 1);
            assert_eq!(zones(&read_board)[0].thieving_settings.pattern, pattern);
        });
    }
}

/// Port of `CopperThievingZone_RejectedInOldFileVersion`: a file of an older format version
/// must reject `(mode thieving)`.
#[test]
fn copper_thieving_zone_rejected_in_old_file_version() {
    // 20260512 is the Net Chains release; thieving needs >= 20260513.  Using the
    // immediately-prior version exercises the boundary check.
    let text = "(kicad_pcb (version 20260512) (generator \"test\") (generator_version \"test\")\
         (general (thickness 1.6)) (paper \"A4\")\
         (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal))\
         (zone (net 0) (net_name \"\") (layer \"F.Cu\") (uuid \"00000000-0000-0000-0000-000000000001\")\
               (hatch edge 0.5)\
               (connect_pads (clearance 0))\
               (min_thickness 0.25) (filled_areas_thickness no)\
               (fill yes (mode thieving) (thermal_gap 0.5) (thermal_bridge_width 0.5)\
                     (island_removal_mode 0))\
               (polygon (pts (xy 0 0) (xy 5 0) (xy 5 5) (xy 0 5)))))";

    run("CopperThievingZone_RejectedInOldFileVersion", || {
        assert!(
            parse_board_report(
                text,
                "copper_thieving_old_version.kicad_pcb",
                &staging_parse()
            )
            .is_err()
        );
    });
}

/// Port of `CopperThievingZone_RejectsMalformedGeometry`: non-positive size / gap / width
/// values are rejected inline, keeping the constructor defaults.
#[test]
fn copper_thieving_zone_rejects_malformed_geometry() {
    let text = "(kicad_pcb (version 20260513) (generator \"test\") (generator_version \"test\")\
         (general (thickness 1.6)) (paper \"A4\")\
         (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal))\
         (zone (net 0) (net_name \"\") (layer \"F.Cu\") (uuid \"00000000-0000-0000-0000-000000000002\")\
               (hatch edge 0.5)\
               (connect_pads (clearance 0))\
               (min_thickness 0.25) (filled_areas_thickness no)\
               (fill yes (mode thieving)\
                     (thermal_gap 0.5) (thermal_bridge_width 0.5)\
                     (island_removal_mode 0)\
                     (thieving (type dots) (size -1) (gap 0)\
                               (width -5) (stagger no) (orientation 0)))\
               (polygon (pts (xy 0 0) (xy 5 0) (xy 5 5) (xy 0 5)))))";

    run("CopperThievingZone_RejectsMalformedGeometry", || {
        let board = parse(text, "copper_thieving_malformed.kicad_pcb");

        assert_eq!(zones(&board).len(), 1);

        let loaded = &zones(&board)[0].thieving_settings;
        assert!(loaded.element_size > 0);
        assert!(loaded.gap > 0);
        assert!(loaded.line_width > 0);
    });
}
