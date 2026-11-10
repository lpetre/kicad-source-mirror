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

//! Test fixture for the FAB-04 crafted boards while the pad parser (PCBIO-07) is pending:
//! the board without its `(footprint ...)` blocks goes through the real parser, the
//! footprints and pads are then built through the model API from the same file text.
//!
//! Only the tokens the crafted boards of `tests/data/plot_items` use are read, with the
//! meaning `PCB_IO_KICAD_SEXPR_PARSER::parseFOOTPRINT_unchecked` / `parsePAD` give them for a
//! front-side footprint: `(transform (translate) (rotate))`, the reference property, and per
//! pad the attribute, shape, `at` (library-frame position, board-frame angle), size, drill,
//! `rect_delta`, layers, `roundrect_rratio`, `chamfer_ratio`, `chamfer`, net (by name, created
//! on first use), local mask / paste margins, `property`, `pinfunction`,
//! `remove_unused_layers` / `keep_end_layers`, the custom `anchor` and `gr_poly` primitives.
//! Once PCBIO-07 lands the crafted boards load directly and this fixture is unused.

#![allow(dead_code)]

use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::{EdaAngle, ShapeLineChain, ShapePolySet, Vector2I};
use kicad_pcb::board::Board;
use kicad_pcb::board::containers::AddMode;
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::ids::NetCode;
use kicad_pcb::item::ParentRef;
use kicad_pcb::item_ref::OwnedItem;
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{PadAttrib, PadProp, PadShape, Padstack, UnconnectedLayerMode};
use kicad_pcb::pcb_shape::PcbShape;

/// A parsed s-expression.
#[derive(Debug, Clone)]
pub enum Sx {
    Atom(String),
    List(Vec<Sx>),
}

impl Sx {
    fn head(&self) -> Option<&str> {
        match self {
            Sx::List(v) => match v.first() {
                Some(Sx::Atom(a)) => Some(a),
                _ => None,
            },
            Sx::Atom(_) => None,
        }
    }

    fn items(&self) -> &[Sx] {
        match self {
            Sx::List(v) => v,
            Sx::Atom(_) => &[],
        }
    }

    fn atom(&self, i: usize) -> &str {
        match &self.items()[i] {
            Sx::Atom(a) => a,
            Sx::List(_) => panic!("atom expected"),
        }
    }

    fn child(&self, name: &str) -> Option<&Sx> {
        self.items().iter().find(|c| c.head() == Some(name))
    }

    fn children<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Sx> + 's {
        self.items().iter().filter(move |c| c.head() == Some(name))
    }
}

/// Reads one s-expression starting at `pos` (quoted strings without escapes).
fn read(text: &[u8], pos: &mut usize) -> Sx {
    while text[*pos].is_ascii_whitespace() {
        *pos += 1;
    }

    if text[*pos] == b'(' {
        *pos += 1;
        let mut v = Vec::new();

        loop {
            while text[*pos].is_ascii_whitespace() {
                *pos += 1;
            }

            if text[*pos] == b')' {
                *pos += 1;
                return Sx::List(v);
            }

            v.push(read(text, pos));
        }
    } else if text[*pos] == b'"' {
        let start = *pos + 1;
        *pos = start;

        while text[*pos] != b'"' {
            *pos += 1;
        }

        *pos += 1;
        Sx::Atom(String::from_utf8(text[start..*pos - 1].to_vec()).unwrap())
    } else {
        let start = *pos;

        while !text[*pos].is_ascii_whitespace() && text[*pos] != b'(' && text[*pos] != b')' {
            *pos += 1;
        }

        Sx::Atom(String::from_utf8(text[start..*pos].to_vec()).unwrap())
    }
}

fn mm(s: &str) -> i32 {
    PCB_IU_SCALE.mm_to_iu(s.parse::<f64>().unwrap())
}

fn xy(sx: &Sx) -> Vector2I {
    Vector2I::new(mm(sx.atom(1)), mm(sx.atom(2)))
}

fn layer(name: &str) -> PcbLayerId {
    PcbLayerId(Lset::name_to_layer(name))
}

/// Splits the board text into the text without the top-level footprints and the footprints.
fn split_footprints(text: &str) -> (String, Vec<Sx>) {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut footprints = Vec::new();
    let mut last = 0;
    let mut search = 0;

    while let Some(rel) = text[search..].find("\n\t(footprint ") {
        let start = search + rel + 1;
        let mut pos = start + 1;
        let sx = read(bytes, &mut pos);
        out.push_str(&text[last..start]);
        footprints.push(sx);
        last = pos;
        search = pos;
    }

    out.push_str(&text[last..]);
    (out, footprints)
}

/// The layer set of a `(layers ...)` list.
fn layers(sx: &Sx) -> Lset {
    let mut set = Lset::new();

    for i in 1..sx.items().len() {
        match sx.atom(i) {
            "*.Cu" => set |= Lset::all_cu_mask(),
            "*.Mask" => {
                set.set(PcbLayerId::FMask);
                set.set(PcbLayerId::BMask);
            }
            "*.Paste" => {
                set.set(PcbLayerId::FPaste);
                set.set(PcbLayerId::BPaste);
            }
            name => {
                set.set(layer(name));
            }
        }
    }

    set
}

/// Builds one pad of a footprint from its `(pad ...)` expression.
fn build_pad(board: &mut Board, sx: &Sx) -> (Pad, Vector2I, Option<EdaAngle>) {
    let mut pad = Pad::default();
    let all = Padstack::ALL_LAYERS;
    let mut ps = pad.lib_padstack().clone();

    pad.number = sx.atom(1).to_string();
    pad.attribute = match sx.atom(2) {
        "thru_hole" => PadAttrib::Pth,
        "smd" => PadAttrib::Smd,
        "connect" => PadAttrib::Conn,
        "np_thru_hole" => PadAttrib::Npth,
        other => panic!("pad type {other}"),
    };
    let shape = match sx.atom(3) {
        "circle" => PadShape::Circle,
        "rect" => PadShape::Rectangle,
        "oval" => PadShape::Oval,
        "trapezoid" => PadShape::Trapezoid,
        "roundrect" => PadShape::Roundrect,
        "custom" => PadShape::Custom,
        other => panic!("pad shape {other}"),
    };

    let at = sx.child("at").unwrap();
    let pos = xy(at);
    let angle = (at.items().len() > 3).then(|| EdaAngle::from_degrees(at.atom(3).parse().unwrap()));

    ps.set_size(xy(sx.child("size").unwrap()), all);

    if let Some(drill) = sx.child("drill") {
        let d = mm(drill.atom(1));
        ps.drill_mut().size = Vector2I::new(d, d);
    } else {
        ps.drill_mut().size = Vector2I::new(0, 0);
    }

    if let Some(delta) = sx.child("rect_delta") {
        *ps.trapezoid_delta_size_mut(all) = xy(delta);
    }

    ps.layer_set = layers(sx.child("layers").unwrap());

    if let Some(r) = sx.child("roundrect_rratio") {
        ps.set_round_rect_radius_ratio(r.atom(1).parse().unwrap(), all);
    }

    let mut final_shape = shape;

    if let Some(r) = sx.child("chamfer_ratio") {
        ps.set_chamfer_ratio(r.atom(1).parse().unwrap(), all);
    }

    if let Some(c) = sx.child("chamfer") {
        let mut bits = 0;

        for i in 1..c.items().len() {
            bits |= match c.atom(i) {
                "top_left" => 1,
                "top_right" => 2,
                "bottom_left" => 4,
                "bottom_right" => 8,
                other => panic!("chamfer {other}"),
            };
        }

        ps.set_chamfer_positions(bits, all);

        if bits != 0 && shape == PadShape::Roundrect {
            final_shape = PadShape::ChamferedRect;
        }
    }

    ps.set_shape(final_shape, all);

    if let Some(options) = sx.child("options")
        && let Some(anchor) = options.child("anchor")
    {
        let anchor_shape = if anchor.atom(1) == "rect" {
            PadShape::Rectangle
        } else {
            PadShape::Circle
        };
        ps.set_anchor_shape(anchor_shape, all);
    }

    if let Some(prims) = sx.child("primitives") {
        for poly in prims.children("gr_poly") {
            let mut chain = ShapeLineChain::new();

            for p in poly.child("pts").unwrap().children("xy") {
                let v = xy(p);
                chain.append_xy(v.x, v.y, false);
            }

            chain.set_closed(true);
            let mut ps_poly = ShapePolySet::new();
            ps_poly.add_outline(&chain);

            let mut prim = PcbShape::default();
            let env = kicad_pcb::ctx::MutEnv::detached();
            prim.set_shape(&env, ShapeT::Poly);
            prim.set_poly_shape(&env, ps_poly.clone());
            prim.override_lib_poly(ps_poly);
            let width = poly.child("width").map_or(0, |w| mm(w.atom(1)));
            prim.set_stroke(
                &env,
                kicad_core::stroke_params::StrokeParams::new(
                    width,
                    kicad_core::stroke_params::LineStyle::Solid,
                    kicad_core::color4d::Color4D::UNSPECIFIED,
                ),
            );
            prim.shape
                .set_filled(poly.child("fill").is_some_and(|f| f.atom(1) == "yes"));
            prim.base.item.parent = ParentRef::None;
            ps.add_primitive(prim, all);
        }
    }

    pad.set_padstack(&ps);

    if let Some(net) = sx.child("net") {
        pad.base.net = board.find_or_add_net(net.atom(1), None);
    } else {
        pad.base.net = NetCode::UNCONNECTED;
    }

    if let Some(m) = sx.child("solder_mask_margin") {
        pad.set_local_solder_mask_margin(Some(mm(m.atom(1))));
    }

    if let Some(m) = sx.child("solder_paste_margin") {
        pad.set_local_solder_paste_margin(Some(mm(m.atom(1))));
    }

    if let Some(m) = sx.child("solder_paste_margin_ratio") {
        pad.set_local_solder_paste_margin_ratio(Some(m.atom(1).parse().unwrap()));
    }

    if let Some(p) = sx.child("property") {
        pad.property = match p.atom(1) {
            "pad_prop_bga" => PadProp::Bga,
            "pad_prop_fiducial_glob" => PadProp::FiducialGlbl,
            "pad_prop_fiducial_loc" => PadProp::FiducialLocal,
            "pad_prop_testpoint" => PadProp::Testpoint,
            "pad_prop_heatsink" => PadProp::Heatsink,
            "pad_prop_castellated" => PadProp::Castellated,
            other => panic!("pad property {other}"),
        };
    }

    if let Some(f) = sx.child("pinfunction") {
        pad.pin_function = f.atom(1).to_string();
    }

    if sx
        .child("remove_unused_layers")
        .is_some_and(|r| r.items().len() < 2 || r.atom(1) == "yes")
    {
        let keep = sx
            .child("keep_end_layers")
            .is_some_and(|r| r.items().len() < 2 || r.atom(1) == "yes");
        pad.set_unconnected_layer_mode(if keep {
            UnconnectedLayerMode::RemoveExceptStartAndEnd
        } else {
            UnconnectedLayerMode::RemoveAll
        });
    }

    (pad, pos, angle)
}

/// Parses `text` (a crafted board) with the footprints built through the model API.
pub fn load_with_fixture_footprints(text: &str, name: &str) -> Board {
    let (rest, footprints) = split_footprints(text);
    let mut board =
        kicad_pcb::io::parse_board(&rest, name, &kicad_pcb::io::ParseOptions::default()).unwrap();
    board.file_name = name.to_string();

    for sx in &footprints {
        assert_eq!(
            sx.child("layer").unwrap().atom(1),
            "F.Cu",
            "front footprints only"
        );
        let transform = sx.child("transform").unwrap();
        let pos = xy(transform.child("translate").unwrap());
        let rot =
            EdaAngle::from_degrees(transform.child("rotate").unwrap().atom(1).parse().unwrap());

        let fp = board.create_footprint();
        board.footprint_set_position(fp, pos);
        board.footprint_set_orientation(fp, rot);

        // The mandatory fields of `create_footprint` (Reference, Value, Datasheet,
        // Description, in this order): text and visibility from the file.
        let field_ids: Vec<_> = board.footprint_fields(fp).map(|(id, _)| id).collect();

        for (i, name) in ["Reference", "Value", "Datasheet", "Description"]
            .iter()
            .enumerate()
        {
            if let Some(p) = sx.children("property").find(|p| p.atom(1) == *name) {
                let text = p.atom(2).to_string();
                let hidden = p.child("hide").is_some_and(|h| h.atom(1) == "yes");
                board.modify_field(field_ids[i], |f, _| {
                    f.text.text.set_text(&text);
                    f.text.text.set_visible(!hidden);
                });
            }
        }

        for pad_sx in sx.children("pad") {
            let (pad, lib_pos, angle) = build_pad(&mut board, pad_sx);
            let id = board
                .footprint_add(fp, OwnedItem::Pad(Box::new(pad)), AddMode::Append)
                .unwrap();
            let pad_id = id.typed::<Pad>().unwrap();
            board.modify_pad(pad_id, |pad, env| {
                pad.set_fp_relative_position(env, lib_pos);
                // the file angle is the board-frame orientation
                pad.set_fp_relative_orientation(env, angle.unwrap_or(rot) - rot);
            });
        }

        board.attach_footprint(fp, AddMode::BulkAppend);
    }

    board
}
