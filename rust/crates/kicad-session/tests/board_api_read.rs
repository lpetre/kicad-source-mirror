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

//! WASM-API-1: the router read API against the reference dump. The golden
//! (`tests/golden/board_api_read.txt`, generator `tests/oracle/gen_board_api_golden.sh`) holds
//! the hashed `pcb-dump board --loader` lines of 32 boards for the fields the API returns;
//! this test formats the API's values the way the dump prints them (PCB_DUMP_FORMAT.md 2)
//! and compares the hashes, keyed by `SECTION|UUID|FIELD`.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use kicad_pcb::dump::format::{v_f64, v_str};
use kicad_qa::io_corpus::fnv1a64;
use kicad_session::board_api::Polygons;
use kicad_session::board_api::read::{BoardInfo, PadInfo};
use kicad_session::{LoadInputs, Session};

fn golden() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/board_api_read.txt"),
    )
    .expect("golden")
}

fn hash(v: &str) -> String {
    format!("#{:016x}", fnv1a64(v.as_bytes()))
}

fn v_pt(p: [i32; 2]) -> String {
    format!("{},{}", p[0], p[1])
}

fn v_chain(pts: &[[i32; 2]], width: i32) -> String {
    let p: Vec<String> = pts.iter().map(|p| v_pt(*p)).collect();
    format!("C(closed w={width} n={} [{}])", pts.len(), p.join(" "))
}

/// `VPolySet` of a [`Polygons`] (no arcs: the API returns vertex lists).
fn v_polys(polys: &Polygons) -> String {
    let mut r = format!("P({}", polys.len());

    for p in polys {
        r.push_str(" {");
        r.push_str(&v_chain(&p.outline, p.width));

        for h in &p.holes {
            r.push_str(" hole ");
            r.push_str(&v_chain(h, 0));
        }

        r.push('}');
    }

    r.push(')');
    r
}

fn v_bool(b: bool) -> String {
    b.to_string()
}

fn v_box(b: [i64; 4]) -> String {
    format!("{},{},{},{}", b[0], b[1], b[2], b[3])
}

fn v_opt(v: Option<i32>) -> String {
    v.map_or("none".to_string(), |v| v.to_string())
}

/// The API's values as `SECTION|KEY|FIELD` -> dump value text.
fn api_lines(s: &mut Session) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut put = |section: &str, key: &str, field: &str, value: String| {
        m.insert(format!("{section}|{key}|{field}"), value);
    };

    let b: BoardInfo = s.board_info();
    put(
        "board",
        "board",
        "copper_layer_count",
        b.copper_layer_count.to_string(),
    );
    put(
        "board",
        "board",
        "enabled_layer_names",
        format!("[{}]", b.enabled_layers.join(",")),
    );

    for l in &b.copper_layers {
        put(
            "board",
            "board",
            &format!("layer.{}.name", l.name),
            v_str(&l.user_name),
        );
        put(
            "board",
            "board",
            &format!("layer.{}.type", l.name),
            format!("LT_{}", l.layer_type.to_ascii_uppercase()),
        );
    }

    put("board", "board", "bbox", v_box(b.bbox));
    put(
        "board",
        "board",
        "outline_inferred.ok",
        v_bool(b.outline_ok),
    );
    put("board", "board", "outline_inferred", v_polys(&b.outline));

    let pads: Vec<PadInfo> = s.pads(true);

    for p in &pads {
        let k = &p.uuid;
        put("item", k, "net", p.net.to_string());
        put("item", k, "locked", v_bool(p.locked));
        put("pad", k, "number", v_str(&p.number));
        put("pad", k, "attribute", p.attribute.clone());
        put("pad", k, "position", v_pt(p.position));
        put("pad", k, "orientation", v_f64(p.orientation));
        if let Some(d) = &p.drill {
            put("pad", k, "drill_shape", d.shape.clone());
        }

        put(
            "pad",
            k,
            "drill_size",
            v_pt(p.drill.as_ref().map_or([0, 0], |d| d.size)),
        );

        for sh in &p.shapes {
            let f = |n: &str| format!("layer.{}.{n}", sh.layer);
            put("pad", k, &f("shape"), sh.shape.clone());
            put("pad", k, &f("size"), v_pt(sh.size));
            put("pad", k, &f("offset"), v_pt(sh.offset));
            put(
                "pad",
                k,
                &f("round_rect_corner_radius"),
                sh.corner_radius.to_string(),
            );
            put("pad", k, &f("chamfer_ratio"), v_f64(sh.chamfer_ratio));
            put(
                "pad",
                k,
                &f("chamfer_positions"),
                sh.chamfer_positions.to_string(),
            );
        }

        for poly in &p.polygons {
            put(
                "pad",
                k,
                &format!("flash.{}", poly.layer),
                v_bool(poly.flashed),
            );
            put(
                "pad",
                k,
                &format!("effective_polygon.{}", poly.layer),
                v_polys(&poly.polygon),
            );
        }
    }

    for t in s.route_items() {
        let k = &t.uuid;
        put("item", k, "net", t.net.to_string());
        put("item", k, "locked", v_bool(t.locked));
        put("track", k, "start", v_pt(t.start));
        put("track", k, "end", v_pt(t.end));

        if let Some(mid) = t.mid {
            put("track", k, "mid", v_pt(mid));
        }

        match &t.via {
            None => put("track", k, "width", t.width.to_string()),
            Some(v) => {
                let vt = match v.via_type.as_str() {
                    "micro" => "MICROVIA".to_string(),
                    other => other.to_ascii_uppercase(),
                };
                put("track", k, "via_type", vt);
                put("track", k, "top_layer", v.top.clone());
                put("track", k, "bottom_layer", v.bottom.clone());
                put("track", k, "drill_value", v.drill.to_string());
                put("track", k, "is_free", v_bool(v.free));

                for (l, w) in &v.diameters {
                    put("track", k, &format!("width.{l}"), w.to_string());
                }
            }
        }
    }

    for f in s.footprints_info() {
        let k = &f.uuid;
        put("item", k, "locked", v_bool(f.locked));
        put("footprint", k, "reference", v_str(&f.reference));
        put("footprint", k, "value", v_str(&f.value));
        put("footprint", k, "position", v_pt(f.position));
        put("footprint", k, "orientation", v_f64(f.orientation));
        put("footprint", k, "side", f.side.clone());
        put("footprint", k, "courtyard.F", v_polys(&f.courtyard_front));
        put("footprint", k, "courtyard.B", v_polys(&f.courtyard_back));
        put("bbox", k, "bbox", v_box(f.bbox));
    }

    for z in s.zones_info(true) {
        let k = &z.uuid;
        put("item", k, "net", z.net.to_string());
        put("zone", k, "name", v_str(&z.name));
        put("zone", k, "priority", z.priority.to_string());
        put("zone", k, "rule_area", v_bool(z.rule_area));
        put(
            "zone",
            k,
            "keepout.zone_fills",
            v_bool(z.keepout.copper_pour),
        );
        put("zone", k, "keepout.vias", v_bool(z.keepout.vias));
        put("zone", k, "keepout.tracks", v_bool(z.keepout.tracks));
        put("zone", k, "keepout.pads", v_bool(z.keepout.pads));
        put(
            "zone",
            k,
            "keepout.footprints",
            v_bool(z.keepout.footprints),
        );
        put("zone", k, "teardrop_area", v_bool(z.teardrop));
        put("zone", k, "local_clearance", v_opt(z.clearance));
        put("zone", k, "min_thickness", z.min_thickness.to_string());
        put("zone", k, "pad_connection", z.connection.clone());
        put("zone", k, "island_removal_mode", z.island_mode.clone());

        if z.footprint.is_none() {
            // `Outline()` is the library frame for a footprint zone; the API returns the
            // board frame.
            put("zone", k, "outline", v_polys(&z.outline));
        }

        for f in &z.fills {
            put(
                "zone",
                k,
                &format!("filled.{}", f.layer),
                v_polys(&f.polygons),
            );
        }
    }

    m
}

fn load(rel: &str) -> Session {
    let path = kicad_qa::kicad_source_root().join(rel);
    let text = std::fs::read_to_string(&path).expect("board");
    let pro = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let dru = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
    let name = path.to_string_lossy().into_owned();
    let inputs = LoadInputs {
        text: &text,
        source_name: &name,
        project_json: pro.as_deref(),
        rules_text: dru.as_deref(),
        ..LoadInputs::default()
    };
    common::load_staged(kicad_pcb::project::LoadPath::Cli, &inputs)
        .expect("load")
        .0
}

#[test]
fn read_api_matches_reference_dump() {
    let golden = golden();
    let mut boards: Vec<(String, BTreeMap<String, String>)> = Vec::new();

    for line in golden.lines() {
        if let Some(rest) = line.strip_prefix("file|") {
            boards.push((rest.trim_end_matches('|').to_string(), BTreeMap::new()));
        } else if let Some((key, value)) = line.split_once('=') {
            if let Some((_, m)) = boards.last_mut() {
                m.insert(key.to_string(), value.to_string());
            }
        }
    }

    assert_eq!(boards.len(), 32);
    let mut compared = 0usize;
    let mut uncovered: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatches = Vec::new();

    for (name, expected) in &boards {
        let mut s = load(name);
        let actual = api_lines(&mut s);

        for (key, hashed) in expected {
            let parts: Vec<&str> = key.splitn(3, '|').collect();

            if parts[1].starts_with('#') {
                continue; // generated UUID: no stable key
            }

            match actual.get(key) {
                Some(v) => {
                    compared += 1;

                    if hash(v) != *hashed {
                        mismatches.push(format!("{name}: {key} = {}", &v[..v.len().min(160)]));
                    }
                }
                None => {
                    let field = parts[2].split('.').next().unwrap_or("").to_string();
                    *uncovered
                        .entry(format!("{}|{field}", parts[0]))
                        .or_default() += 1;
                }
            }
        }
    }

    eprintln!("compared {compared} values; not returned by the API: {uncovered:?}");
    assert!(compared > 15_000, "{compared}");
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches[..mismatches.len().min(40)].join("\n")
    );
}
