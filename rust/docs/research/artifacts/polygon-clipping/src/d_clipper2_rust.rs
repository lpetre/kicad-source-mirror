// Differential driver: clipper2-rust 1.2.0 (feature `using_z`). Scratch research code.
use clipper2_rust::{
    ClipType, Clipper64, ClipperOffset, EndType, FillRule, JoinType, Path64, Paths64, Point64,
    PolyTree64, Rect64, area, minkowski_diff, minkowski_sum, rect_clip_64, simplify_paths,
};
use crate::common::{Case, P3, put_path};
use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

fn to_path(p: &[P3]) -> Path64 {
    p.iter().map(|&(x, y, z)| Point64::new_z(x, y, z)).collect()
}

fn to_paths(p: &[Vec<P3>]) -> Paths64 {
    p.iter().map(|q| to_path(q)).collect()
}

fn dump_path(out: &mut String, tag: &str, p: &Path64) {
    put_path(out, tag, p.iter().map(|pt| (pt.x, pt.y, pt.z)), p.len());
}

fn dump_paths(out: &mut String, title: &str, paths: &Paths64) {
    writeln!(out, "{} {}", title, paths.len()).unwrap();
    for p in paths {
        dump_path(out, "Q", p);
    }
}

fn dump_node(out: &mut String, tree: &PolyTree64, id: usize, depth: usize) {
    let node = &tree.nodes[id];
    let tag = format!("N {} {}", depth, if tree.is_hole(id) { 1 } else { 0 });
    dump_path(out, &tag, node.polygon());
    for &c in node.children() {
        dump_node(out, tree, c, depth + 1);
    }
}

fn dump_tree(out: &mut String, title: &str, tree: &PolyTree64) {
    writeln!(out, "{}", title).unwrap();
    for &r in tree.root().children() {
        dump_node(out, tree, r, 0);
    }
}

fn ct(v: i64) -> ClipType {
    match v {
        1 => ClipType::Intersection,
        2 => ClipType::Union,
        3 => ClipType::Difference,
        4 => ClipType::Xor,
        _ => ClipType::NoClip,
    }
}

fn fr(v: i64) -> FillRule {
    match v {
        0 => FillRule::EvenOdd,
        1 => FillRule::NonZero,
        2 => FillRule::Positive,
        _ => FillRule::Negative,
    }
}

fn jt(v: i64) -> JoinType {
    match v {
        0 => JoinType::Square,
        1 => JoinType::Bevel,
        2 => JoinType::Round,
        _ => JoinType::Miter,
    }
}

fn et(v: i64) -> EndType {
    match v {
        0 => EndType::Polygon,
        1 => EndType::Joined,
        2 => EndType::Butt,
        3 => EndType::Square,
        _ => EndType::Round,
    }
}

fn run_bool(c: &Case, out: &mut String) {
    let (ctv, frv, pc) = (ct(c.arg_i64(0)), fr(c.arg_i64(1)), c.arg_i64(2) != 0);
    let subj = to_paths(&c.subj);
    let clip = to_paths(&c.clip);
    for (tag, tree_mode) in [("Z", true), ("Y", false)] {
        let mut clipper = Clipper64::new();
        clipper.set_preserve_collinear(pc);
        clipper.add_subject(&subj);
        clipper.add_clip(&clip);
        let log = Rc::new(RefCell::new(String::new()));
        let next_z = Rc::new(Cell::new(c.npoints));
        {
            let log = log.clone();
            clipper.set_z_callback(
                move |e1bot: &Point64, e1top: &Point64, e2bot: &Point64, e2top: &Point64, pt: &mut Point64| {
                    let mut l = log.borrow_mut();
                    l.push_str(tag);
                    for p in [*e1bot, *e1top, *e2bot, *e2top, *pt] {
                        write!(l, " {} {} {}", p.x, p.y, p.z).unwrap();
                    }
                    l.push('\n');
                    pt.z = next_z.get();
                    next_z.set(next_z.get() + 1);
                },
            );
        }
        if tree_mode {
            let mut tree = PolyTree64::new();
            let mut open = Paths64::new();
            let ok = clipper.execute_tree(ctv, frv, &mut tree, &mut open);
            out.push_str(&log.borrow());
            writeln!(out, "OK {}", if ok { 1 } else { 0 }).unwrap();
            dump_tree(out, "TREE", &tree);
        } else {
            let mut closed = Paths64::new();
            let ok = clipper.execute(ctv, frv, &mut closed, None);
            out.push_str(&log.borrow());
            writeln!(out, "OK {}", if ok { 1 } else { 0 }).unwrap();
            dump_paths(out, "PATHS", &closed);
        }
    }
}

fn setup_offset(c: &Case) -> ClipperOffset {
    let mut co = ClipperOffset::new_default();
    for g in &c.groups {
        co.add_paths(&to_paths(g), jt(c.arg_i64(0)), et(c.arg_i64(1)));
    }
    co.set_arc_tolerance(c.arg_f64(3));
    co.set_miter_limit(c.arg_f64(2));
    co
}

fn run_offset(c: &Case, out: &mut String) {
    let delta = c.arg_f64(4);
    {
        let mut co = setup_offset(c);
        let mut tree = PolyTree64::new();
        co.execute_tree(delta, &mut tree);
        dump_tree(out, "TREE", &tree);
    }
    {
        let mut co = setup_offset(c);
        let mut paths = Paths64::new();
        co.execute(delta, &mut paths);
        dump_paths(out, "PATHS", &paths);
        let mut c2 = Clipper64::new();
        c2.set_preserve_collinear(false);
        c2.set_reverse_solution(false);
        c2.add_subject(&paths);
        let mut tree = PolyTree64::new();
        let mut open = Paths64::new();
        c2.execute_tree(ClipType::Union, FillRule::Positive, &mut tree, &mut open);
        dump_tree(out, "TREE2", &tree);
    }
}

pub fn run(c: &Case, out: &mut String) {
    match c.op.as_str() {
        "BOOL" => run_bool(c, out),
        "OFFSET" => run_offset(c, out),
        "SIMPLIFY" => {
            let res = simplify_paths(&to_paths(&c.subj), c.arg_f64(0), c.arg_i64(1) != 0);
            dump_paths(out, "PATHS", &res);
        }
        "AREA" => {
            for p in &c.subj {
                writeln!(out, "A {}", area(&to_path(p)).to_bits()).unwrap();
            }
        }
        "RECTCLIP" => {
            let r = Rect64::new(c.arg_i64(0), c.arg_i64(1), c.arg_i64(2), c.arg_i64(3));
            dump_paths(out, "PATHS", &rect_clip_64(&r, &to_paths(&c.subj)));
        }
        "MINKSUM" => {
            let p = minkowski_sum(&to_path(&c.subj[0]), &to_path(&c.clip[0]), c.arg_i64(0) != 0);
            dump_paths(out, "PATHS", &p);
        }
        "MINKDIFF" => {
            let p = minkowski_diff(&to_path(&c.subj[0]), &to_path(&c.clip[0]), c.arg_i64(0) != 0);
            dump_paths(out, "PATHS", &p);
        }
        _ => out.push_str("UNKNOWN\n"),
    }
}

