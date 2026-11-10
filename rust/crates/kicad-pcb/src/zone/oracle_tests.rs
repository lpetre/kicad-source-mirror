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

//! Package M18 differential test (new code): replays `tests/golden/m18_zones.txt`, produced by
//! the C++ driver `tests/oracle/m18_zone_oracle.cpp` (`m18_zone_golden.sh`) on
//! `tests/oracle/m18_zones.kicad_pcb`. The zones are rebuilt from their dumped members (the
//! zone parser is a later package), every derived value of `zone.cpp` / `zone_utils.cpp` the
//! driver prints is recomputed and compared as text.

use std::collections::BTreeMap;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{EdaUnits, Kiid, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::shape_poly_set::VertexIndex;
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{
    Box2I, EdaAngle, FlipDirection, Seg, ShapeLineChain, ShapePolySet, ShapeType, Vector2I,
};

use crate::board::{AddMode, Board};
use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::footprint::Footprint;
use crate::ids::{FootprintId, NetCode, ZoneId};
use crate::item::{DetachedCopy, ParentRef};
use crate::teardrop_parameters::TeardropType;
use crate::traits::Flashing;
use crate::zone::Zone;
use crate::zone_settings::{CornerSmoothing, ZoneBorderDisplayStyle};
use crate::zone_utils::{auto_assign_zone_priorities, merge_zones_with_same_outline};

// ---- the pcb-dump value formats (copies of the `dump` feature's helpers, arc-free) -----------

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for &c in data {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    h
}

fn v_int(v: impl Into<i64>) -> String {
    v.into().to_string()
}

fn v_bool(v: bool) -> String {
    if v { "true" } else { "false" }.to_string()
}

/// `V( double )`: `%.17g`.
fn v_f64(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }

    let e = format!("{x:.16e}");
    let (mant, exp) = e.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent value");
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let sign = if neg { "-" } else { "" };
    let strip = |s: &mut String| {
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
    };

    if !(-4..17).contains(&exp) {
        let mut m = format!("{}.{}", &digits[..1], &digits[1..]);
        strip(&mut m);
        let esign = if exp < 0 { '-' } else { '+' };
        return format!("{sign}{m}e{esign}{:02}", exp.abs());
    }

    let mut s = if exp >= 0 {
        let int_len = (exp + 1) as usize;
        format!("{}.{}", &digits[..int_len], &digits[int_len..])
    } else {
        format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    };
    strip(&mut s);
    format!("{sign}{s}")
}

fn v_str(s: &str) -> String {
    format!("\"{s}\"")
}

fn v_vec(v: Vector2I) -> String {
    format!("{},{}", v.x, v.y)
}

fn v_box(b: Box2I) -> String {
    let p = b.position();
    let s = b.size();
    format!("{},{},{},{}", p.x, p.y, s.x as i32, s.y as i32)
}

fn v_angle(a: EdaAngle) -> String {
    v_f64(a.as_degrees())
}

fn v_opt_int(v: Option<i32>) -> String {
    v.map_or_else(|| "none".to_string(), |v| v.to_string())
}

fn v_layer(layer: PcbLayerId) -> String {
    Board::standard_layer_name(layer)
}

/// `V( LSET )`: 32 hex digits, most significant first.
fn v_lset(s: &Lset) -> String {
    (0..32)
        .rev()
        .map(|nib| {
            let v = (0..4).fold(0u32, |acc, b| {
                acc | (u32::from(s.test(PcbLayerId(nib * 4 + b))) << b)
            });
            char::from(b"0123456789abcdef"[v as usize])
        })
        .collect()
}

fn v_chain(c: &ShapeLineChain) -> String {
    assert_eq!(c.arc_count(), 0, "arc-free chains only");
    let pts: Vec<String> = c.c_points().iter().map(|p| v_vec(*p)).collect();
    format!(
        "C({} w={} n={} [{}])",
        if c.is_closed() { "closed" } else { "open" },
        c.width(),
        c.point_count(),
        pts.join(" ")
    )
}

fn v_polyset(s: &ShapePolySet) -> String {
    let mut r = format!("P({}", s.outline_count());

    for i in 0..s.outline_count() {
        r.push_str(" {");
        r.push_str(&v_chain(s.c_outline(i)));

        for h in 0..s.hole_count(i) {
            r.push_str(" hole ");
            r.push_str(&v_chain(s.c_hole(i, h)));
        }

        r.push('}');
    }

    r.push(')');
    r
}

const GOLDEN: &str = include_str!("../../tests/golden/m18_zones.txt");

/// `SECTION|KEY|FIELD=VALUE` lines grouped by `(section, key)`.
fn golden() -> BTreeMap<(String, String), BTreeMap<String, String>> {
    let mut map: BTreeMap<(String, String), BTreeMap<String, String>> = BTreeMap::new();

    for line in GOLDEN.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(section), Some(key), Some(rest)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let (field, value) = rest.split_once('=').expect("FIELD=VALUE");
        map.entry((section.to_string(), key.to_string()))
            .or_default()
            .insert(field.to_string(), value.to_string());
    }

    map
}

fn vec(s: &str) -> Vector2I {
    let (x, y) = s.split_once(',').expect("x,y");
    Vector2I::new(x.parse().unwrap(), y.parse().unwrap())
}

/// The `V( LSET )` hex text back to a set (rightmost digit = layers 0..3).
fn lset(hex: &str) -> Lset {
    let mut set = Lset::new();

    for (i, c) in hex.chars().rev().enumerate() {
        let nibble = c.to_digit(16).expect("hex digit");

        for bit in 0..4 {
            if nibble & (1 << bit) != 0 {
                set.set(PcbLayerId((i * 4 + bit) as i32));
            }
        }
    }

    set
}

/// The `VPolySet` text of an arc-free polygon set back to a set.
fn polyset(text: &str) -> ShapePolySet {
    let mut set = ShapePolySet::new();

    for poly in text.split('{').skip(1) {
        for (n, chain) in poly.split("C(").skip(1).enumerate() {
            let pts = &chain[chain.find('[').unwrap() + 1..chain.find(']').unwrap()];
            let mut c = ShapeLineChain::new();

            for p in pts.split(' ').filter(|p| !p.is_empty()) {
                c.append_point(vec(p), true);
            }

            c.set_closed(chain.starts_with("closed"));

            if n == 0 {
                set.add_outline(&c);
            } else {
                set.add_hole(&c, -1);
            }
        }
    }

    set
}

fn hash(set: &ShapePolySet) -> String {
    format!("#{:016x}", fnv1a64(v_polyset(set).as_bytes()))
}

fn segs(segs: &[Seg]) -> String {
    let text: Vec<String> = segs
        .iter()
        .map(|s| format!("{}>{}", v_vec(s.a), v_vec(s.b)))
        .collect();
    format!(
        "n={} #{:016x}",
        segs.len(),
        fnv1a64(format!("[{}]", text.join(" ")).as_bytes())
    )
}

fn key_of(uuid: &Kiid) -> String {
    let s = uuid.as_string();
    s[s.len() - 2..].to_string()
}

fn vi(v: &VertexIndex) -> String {
    format!("{},{},{}", v.polygon, v.contour, v.vertex)
}

fn shape_type_int(t: ShapeType) -> i32 {
    t as i32
}

struct Fixture {
    board: Board,
    /// Every zone in the driver's order: board zones, then footprint zones.
    zones: Vec<(String, ZoneId)>,
}

fn build(golden: &BTreeMap<(String, String), BTreeMap<String, String>>) -> Fixture {
    let mut board = Board::new();
    board.set_copper_layer_count(4);
    assert_eq!(board.find_or_add_net("GND", None), NetCode(1));
    assert_eq!(board.find_or_add_net("VCC", None), NetCode(2));

    let mut footprints: BTreeMap<String, FootprintId> = BTreeMap::new();

    for ((section, key), f) in golden {
        if section != "fp" {
            continue;
        }

        let (sx, sy) = f["scale"].split_once(',').unwrap();
        let mut footprint = Footprint::default();
        footprint.base.uuid = Kiid::parse(&f["uuid"]).unwrap();
        footprint.base.layer = if f["layer"] == "B.Cu" {
            PcbLayerId::BCu
        } else {
            PcbLayerId::FCu
        };
        footprint.transform = TransformTrs::new(
            vec(&f["translate"]),
            EdaAngle::from_degrees(f["rotate"].parse().unwrap()),
            sx.parse().unwrap(),
            sy.parse().unwrap(),
        );
        footprint.flipped = f["flipped"] == "true";
        let id = board.insert_item(footprint, ParentRef::Board);
        board.footprint_list.push_back(id);
        footprints.insert(key.clone(), id);
    }

    // The driver's order: board zones (file order), then footprint zones.
    let order: Vec<String> = GOLDEN
        .lines()
        .filter_map(|l| l.strip_prefix("zone|"))
        .filter_map(|l| l.split_once("|parent=").map(|(k, _)| k.to_string()))
        .collect();

    let mut zones = Vec::new();

    for key in order {
        let f = &golden[&("zone".to_string(), key.clone())];
        let mut zone = Zone::default();
        zone.base.item.uuid = Kiid::parse(&f["uuid"]).unwrap();
        zone.base.net = NetCode(f["net"].parse().unwrap());
        zone.priority = f["priority"].parse().unwrap();
        zone.is_rule_area = f["rule_area"] == "true";
        zone.teardrop_type = match f["teardrop"].as_str() {
            "0" => TeardropType::TdNone,
            "1" => TeardropType::TdViapad,
            _ => TeardropType::TdTrackend,
        };
        zone.corner_smoothing_type = match f["smoothing"].as_str() {
            "0" => CornerSmoothing::NoSmoothing,
            "1" => CornerSmoothing::Chamfer,
            _ => CornerSmoothing::Fillet,
        };
        zone.corner_radius = f["radius"].parse().unwrap();
        zone.zone_min_thickness = f["min_thickness"].parse().unwrap();
        zone.zone_clearance = f["clearance"].parse().unwrap();
        zone.zone_name = f["name"].trim_matches('"').to_string();
        let keepout: Vec<bool> = f["keepout"].split(',').map(|s| s == "true").collect();
        zone.do_not_allow_zone_fills = keepout[0];
        zone.do_not_allow_vias = keepout[1];
        zone.do_not_allow_tracks = keepout[2];
        zone.do_not_allow_pads = keepout[3];
        zone.do_not_allow_footprints = keepout[4];
        *zone.lib_outline_mut() = polyset(&f["lib_outline"]);

        let style = match f["border_style"].as_str() {
            "0" => ZoneBorderDisplayStyle::NoHatch,
            "1" => ZoneBorderDisplayStyle::DiagonalFull,
            "2" => ZoneBorderDisplayStyle::DiagonalEdge,
            _ => ZoneBorderDisplayStyle::InvisibleBorder,
        };
        let pitch: i32 = f["border_pitch"].parse().unwrap();
        let layers = lset(&f["layers"]);

        let id = if f["parent"] == "none" {
            let env = board.child_env(ParentRef::Board);
            zone.set_layer_set(&env, layers);
            zone.set_border_display_style(&env, style, pitch, true);
            board.add_zone(zone, AddMode::Append)
        } else {
            let fp = footprints[&f["parent"]];
            let id = board.insert_item(zone, ParentRef::Item(fp.into()));
            board.footprints.get_mut(fp).unwrap().zones.push_back(id);
            board.modify_zone(id, |zone, env| {
                zone.set_layer_set(env, layers);
                zone.set_border_display_style(env, style, pitch, true);
            });
            id
        };

        // The parser's fill state: `(fill yes)` on every zone but the rule areas, no refill
        // pending after loading.
        let filled = f["rule_area"] != "true";
        let fill = board.zone_fill_mut(id);
        fill.set_is_filled(filled);
        fill.set_need_refill(false);

        zones.push((key, id));
    }

    board.check_invariants().unwrap();
    Fixture { board, zones }
}

/// The values the driver prints for one zone.
fn derived(fx: &Fixture, id: ZoneId) -> BTreeMap<String, String> {
    let board = &fx.board;
    let ctx = BoardCtx::bare(board);
    let z = &board[id];
    let env = board.mut_env(id.into());
    let max_error = board.design_settings().max_error();
    let mut d = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        d.insert(k.to_string(), v);
    };

    put("first_layer", v_int(z.first_layer().0));
    put("layer", v_int(z.layer(&ctx).0));
    put("on_copper", v_bool(z.is_on_copper_layer(&ctx)));
    put("bbox", v_box(z.bounding_box(&ctx)));
    put("board_outline", v_polyset(&z.board_outline(&ctx)));
    put("hatch", segs(&z.hatch_lines(&ctx)));
    put("position", v_vec(z.position(&ctx)));
    put("local_clearance", v_opt_int(z.local_clearance()));
    put(
        "local_clearance_src",
        v_opt_int(z.local_clearance_with_source(false).map(|(c, _)| c)),
    );
    put("friendly", v_str(&z.friendly_name(&ctx)));
    let units = UnitsProvider::new(&PCB_IU_SCALE, EdaUnits::Mm);
    put("desc", v_str(&z.item_description(&ctx, &units, true)));
    put(
        "outline_area",
        v_f64(z.detached_copy().calculate_outline_area(&ctx)),
    );
    put("num_corners", v_int(z.num_corners()));

    let mut board_edge = ShapePolySet::new();
    board_edge.new_outline();
    for (x, y) in [(95.0, 95.0), (129.0, 95.0), (129.0, 150.0), (95.0, 150.0)] {
        board_edge.append(
            PCB_IU_SCALE.mm_to_iu(x),
            PCB_IU_SCALE.mm_to_iu(y),
            -1,
            -1,
            false,
        );
    }

    for l in z.layer_set.seq().iter().copied() {
        let ln = v_layer(l);
        let mut smoothed = ShapePolySet::new();
        let mut apron = ShapePolySet::new();
        let mut smoothed_bo = ShapePolySet::new();

        let ok = z.build_smoothed_poly(&ctx, &mut smoothed, l, None, Some(&mut apron));
        put(
            &format!("smoothed.{ln}"),
            format!("{} {}", v_bool(ok), hash(&smoothed)),
        );
        put(&format!("apron.{ln}"), hash(&apron));
        z.build_smoothed_poly(&ctx, &mut smoothed_bo, l, Some(&board_edge), None);
        put(&format!("smoothed_bo.{ln}"), hash(&smoothed_bo));

        let mut tso = ShapePolySet::new();
        z.transform_smoothed_outline_to_polygon(
            &ctx,
            &mut tso,
            l,
            PCB_IU_SCALE.mm_to_iu(0.2),
            max_error,
            ErrorLoc::Outside,
            None,
        );
        put(&format!("tso.{ln}"), hash(&tso));
        let mut tso0 = ShapePolySet::new();
        z.transform_smoothed_outline_to_polygon(
            &ctx,
            &mut tso0,
            l,
            0,
            max_error,
            ErrorLoc::Inside,
            Some(&board_edge),
        );
        put(&format!("tso0.{ln}"), hash(&tso0));

        let (same, other) = z.interacting_zones(&ctx, l);
        let mut s = String::new();
        for (_, o) in &same {
            s += &format!("{} ", key_of(&o.base.item.uuid));
        }
        s += "|";
        for (_, o) in &other {
            s += &format!(" {}", key_of(&o.base.item.uuid));
        }
        put(&format!("interacting.{ln}"), s);
    }

    // ---- hit tests ----
    let bb = z.bounding_box(&ctx);
    let c = bb.centre();
    let bo = z.board_outline(&ctx);
    let v0 = bo.c_vertex_global(0);
    let v1 = bo.c_vertex_global(1);
    let mid = (v0 + v1) / 2;
    let pts = [
        v0,
        mid,
        c,
        v0 + Vector2I::new(150000, 0),
        mid + Vector2I::new(0, 80000),
        Vector2I::new(0, 0),
    ];
    let mut hits = String::new();
    for p in pts {
        hits.push(if z.hit_test(&ctx, p, 0) { '1' } else { '0' });
        hits.push(if z.hit_test(&ctx, p, 200000) {
            '1'
        } else {
            '0'
        });
        hits.push(if z.hit_test_filled_area(&ctx, z.first_layer(), p, 0) {
            '1'
        } else {
            '0'
        });
        hits.push(if z.hit_test_cutout(&ctx, p).is_some() {
            '1'
        } else {
            '0'
        });
        hits.push(',');
    }
    put("hits", hits);

    let mut index = VertexIndex::default();
    let corner = z.hit_test_for_corner(&ctx, v1 + Vector2I::new(1000, 0), 5000);
    if let Some(v) = corner {
        index = v;
    }
    put(
        "corner_hit",
        format!("{} {}", v_bool(corner.is_some()), vi(&index)),
    );
    let edge = z.hit_test_for_edge(&ctx, mid + Vector2I::new(0, 1000), 5000);
    if let Some(v) = edge {
        index = v;
    }
    put(
        "edge_hit",
        format!("{} {}", v_bool(edge.is_some()), vi(&index)),
    );

    let mut big = bb;
    big.inflate(1000);
    let small = Box2I::new(
        c - Vector2I::new(500000, 500000),
        Vector2I::new(1000000, 1000000),
    );
    let corner_box = Box2I::new(
        v0 - Vector2I::new(100000, 100000),
        Vector2I::new(200000, 200000),
    );
    let b = |x: bool| if x { "1" } else { "0" };
    put(
        "hit_rect",
        [
            b(z.hit_test_rect(&ctx, &big, true, 0)),
            b(z.hit_test_rect(&ctx, &bb, true, 0)),
            b(z.hit_test_rect(&ctx, &small, false, 0)),
            b(z.hit_test_rect(&ctx, &corner_box, false, 0)),
            b(z.hit_test_rect(&ctx, &small, true, 0)),
        ]
        .concat(),
    );

    let chain = |pts: &[Vector2I]| {
        let mut c = ShapeLineChain::new();
        for p in pts {
            c.append_point(*p, false);
        }
        c.set_closed(true);
        c
    };
    let sel = chain(&[
        bb.origin() - Vector2I::new(1000, 1000),
        Vector2I::new(bb.right() + 1000, bb.top() - 1000),
        Vector2I::new(bb.right() + 1000, bb.bottom() + 1000),
        Vector2I::new(bb.left() - 1000, bb.bottom() + 1000),
    ]);
    let sel2 = chain(&[
        c,
        c + Vector2I::new(300000, 0),
        c + Vector2I::new(300000, 300000),
    ]);
    let sel3 = chain(&[
        v0 - Vector2I::new(50000, 50000),
        v0 + Vector2I::new(50000, -50000),
        v0 + Vector2I::new(50000, 50000),
    ]);
    put(
        "hit_poly",
        [
            b(z.hit_test_poly(&ctx, &sel, true)),
            b(z.hit_test_poly(&ctx, &sel, false)),
            b(z.hit_test_poly(&ctx, &sel2, true)),
            b(z.hit_test_poly(&ctx, &sel2, false)),
            b(z.hit_test_poly(&ctx, &sel3, false)),
        ]
        .concat(),
    );

    // ---- relations ----
    let mut hp = String::new();
    let mut eq = String::new();
    for (_, other) in &fx.zones {
        let o = &board[*other];
        hp.push(if z.higher_priority(o) { '1' } else { '0' });
        eq.push(if z.eq_item(&ctx, o) { '1' } else { '0' });
    }
    put("higher_priority", hp);
    put("equal", eq);

    // ---- transforms (detached copies keep the parent) ----
    {
        let mut m = z.detached_copy();
        m.move_by(&env, Vector2I::new(1000000, -2000000));
        put("move.outline", hash(m.outline()));
        put("move.hatch", segs(&m.hatch_lines(&ctx)));
        put("move.bbox", v_box(m.bounding_box(&ctx)));
    }
    {
        let mut r = z.detached_copy();
        r.rotate(&env, c, EdaAngle::from_degrees(30.0));
        put("rotate.outline", hash(r.outline()));
        put("rotate.hatch", segs(&r.hatch_lines(&ctx)));
        put("rotate.bbox", v_box(r.bounding_box(&ctx)));
    }
    {
        let mut f = z.detached_copy();
        f.fill
            .set_filled_polys_list(z.first_layer(), &z.board_outline(&ctx));
        f.flip(&env, c, FlipDirection::LeftRight);
        put("flip.outline", hash(f.outline()));
        put("flip.layers", v_lset(&f.layer_set));
        put("flip.hatch", segs(&f.hatch_lines(&ctx)));
        put("flip.hatch_orientation", v_angle(f.hatch_orientation()));
        put("flip.bbox", v_box(f.bounding_box(&ctx)));
        let mut fl = String::new();
        for l in f.filled_layers() {
            fl += &format!("{}:{} ", v_layer(l), hash(&f.filled_polys_list(l)));
        }
        put("flip.fills", fl);
    }
    {
        let mut m = z.detached_copy();
        m.mirror(&env, c, FlipDirection::TopBottom);
        put("mirror.outline", hash(m.outline()));
        put("mirror.hatch", segs(&m.hatch_lines(&ctx)));
        put("mirror.layers", v_lset(&m.layer_set));
    }

    // ---- fill containers ----
    {
        let mut f = z.detached_copy();
        let l = z.first_layer();
        let fill = z.board_outline(&ctx);

        put("fill.has_before", v_bool(f.has_filled_polys_for_layer(l)));
        put("fill.hash_before", f.hash_value(l).to_string_hex());
        f.fill.set_filled_polys_list(l, &fill);
        f.fill.set_is_filled(true);
        f.fill.build_hash_value(l);
        put("fill.hash", f.hash_value(l).to_string_hex());
        put("fill.area", v_f64(f.calculate_filled_area()));

        let tsp = |clearance: i32, loc: ErrorLoc| {
            let mut buf = ShapePolySet::new();
            f.transform_shape_to_polygon(&ctx, &mut buf, l, clearance, max_error, loc, false);
            hash(&buf)
        };
        put(
            "fill.tsp",
            format!(
                "{} {} {} {}",
                tsp(0, ErrorLoc::Inside),
                tsp(100000, ErrorLoc::Outside),
                tsp(100000, ErrorLoc::Inside),
                tsp(-100000, ErrorLoc::Outside)
            ),
        );

        let mut solid = ShapePolySet::new();
        f.transform_solid_areas_shapes_to_polygon(l, &mut solid);
        put("fill.solid", hash(&solid));

        let es = f.effective_shape(&ctx, l, Flashing::Default, DrcConstraintT::NullConstraint);
        let es_hash = match es.as_shape_ref() {
            kicad_geom::ShapeRef::PolySet(p) => hash(p),
            _ => "-".to_string(),
        };
        put(
            "fill.effective",
            format!("{} {}", shape_type_int(es.type_()), es_hash),
        );
        let es2 = f.effective_shape(
            &ctx,
            PcbLayerId::BMask,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        put("fill.effective_other", v_int(shape_type_int(es2.type_())));
        put(
            "fill.hit_filled",
            v_bool(f.hit_test_filled_area(&ctx, l, c, 0)),
        );

        let changed = f.unfill();
        put(
            "fill.unfill",
            format!(
                "{} {} {} {}",
                v_bool(changed),
                v_bool(f.is_filled()),
                v_bool(f.has_filled_polys_for_layer(l)),
                f.filled_polys_list(l).outline_count()
            ),
        );
        put("fill.unfill_again", v_bool(f.unfill()));
    }

    d
}

/// Package M18 differential test: every zone of `m18_zones.kicad_pcb` (copper, multi-layer
/// with a hole, rule areas, teardrop, non-copper, same-net / different-net overlaps incl.
/// the #12797 / #13915 wrinkles, zones in a rotated + non-uniformly scaled and in a flipped
/// footprint) against the reference build.
#[test]
fn oracle_zones() {
    let golden = golden();
    let mut fx = build(&golden);
    let mut checked = 0;
    let mut mismatches = Vec::new();

    for (key, id) in &fx.zones {
        let expected = &golden[&("zone".to_string(), key.clone())];
        let actual = derived(&fx, *id);

        for (field, value) in &actual {
            let want = expected
                .get(field)
                .unwrap_or_else(|| panic!("zone {key}: no golden field {field}"));
            checked += 1;

            if want != value {
                mismatches.push(format!("zone|{key}|{field}: C++ {want} / Rust {value}"));
            }
        }
    }

    // MergeZonesWithSameOutline on copies of the board zones (each with its outline as fill).
    {
        let ctx = BoardCtx::bare(&fx.board);
        let copies: Vec<Zone> = fx
            .board
            .zones()
            .map(|(_, z)| {
                let mut copy = z.detached_copy();
                copy.fill
                    .set_filled_polys_list(z.first_layer(), &z.board_outline(&ctx));
                copy
            })
            .collect();
        let merged = merge_zones_with_same_outline(copies);
        let count = format!("n={}", merged.len());
        checked += 1;
        if golden[&("merge".to_string(), "count".to_string())]["n"] != count[2..] {
            mismatches.push(format!("merge count {count}"));
        }

        for z in &merged {
            let mut fl = String::new();
            for l in z.filled_layers() {
                fl += &format!("{}:{} ", v_layer(l), hash(&z.filled_polys_list(l)));
            }
            let actual = format!(
                "{} filled={} refill={} fills={}",
                v_lset(&z.layer_set),
                v_bool(z.is_filled()),
                v_bool(z.need_refill()),
                fl
            );
            let want = &golden[&("merge".to_string(), key_of(&z.base.item.uuid))]["layers"];
            checked += 1;
            if *want != actual {
                mismatches.push(format!(
                    "merge|{}: C++ {want} / Rust {actual}",
                    key_of(&z.base.item.uuid)
                ));
            }
        }
    }

    // AutoAssignZonePriorities.
    let changed = auto_assign_zone_priorities(&mut fx.board);
    checked += 1;
    if golden[&("auto".to_string(), "result".to_string())]["changed"] != v_bool(changed) {
        mismatches.push(format!("auto changed {changed}"));
    }

    for (_, z) in fx.board.zones() {
        let key = key_of(&z.base.item.uuid);
        checked += 1;
        if golden[&("auto".to_string(), key.clone())]["priority"]
            != z.assigned_priority().to_string()
        {
            mismatches.push(format!(
                "auto|{key}: C++ {} / Rust {}",
                golden[&("auto".to_string(), key.clone())]["priority"],
                z.assigned_priority()
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {checked} values differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert!(checked > 750, "only {checked} values checked");
}
