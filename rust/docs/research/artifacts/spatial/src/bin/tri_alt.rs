//! Smoke test of polygon-triangulation crates noted as alternatives to KiCad's own
//! `polygon_triangulation.h` (they do NOT reproduce KiCad's triangles; see the report).
use i_triangle::float::triangulatable::Triangulatable;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

fn main() {
    // 20 mm square with a 4 mm square hole, coordinates in nm (as f64)
    let outer = [[0.0, 0.0], [20e6, 0.0], [20e6, 20e6], [0.0, 20e6]];
    let hole = [[8e6, 8e6], [8e6, 12e6], [12e6, 12e6], [12e6, 8e6]];

    let flat: Vec<f64> = outer.iter().chain(hole.iter()).flat_map(|p| [p[0], p[1]]).collect();
    let t = earcutr::earcut(&flat, &[4], 2).unwrap();
    println!("earcutr   0.5.0 : {} triangles, first {:?}", t.len() / 3, &t[..3]);

    let mut e = earcut::Earcut::new();
    let mut out: Vec<u32> = Vec::new();
    e.earcut(outer.iter().chain(hole.iter()).copied(), &[4u32], &mut out);
    println!("earcut    0.4.12: {} triangles, first {:?}", out.len() / 3, &out[..3]);

    let shape = vec![outer.to_vec(), hole.to_vec()];
    let tri = shape.triangulate().to_triangulation::<u32>();
    println!("i_triangle 0.51 : {} triangles, first {:?}", tri.indices.len() / 3, &tri.indices[..3]);

    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    cdt.add_constraint_edges(outer.iter().map(|p| Point2::new(p[0], p[1])), true).unwrap();
    cdt.add_constraint_edges(hole.iter().map(|p| Point2::new(p[0], p[1])), true).unwrap();
    println!("spade     2.15.1: {} faces in the CDT (hole not removed)", cdt.num_inner_faces());

    // bit sets
    let mut bv = bitvec::bitvec![u32, bitvec::order::Lsb0; 0; 128];
    bv.set(31, true);
    let mut bs = bit_set::BitSet::with_capacity(128);
    bs.insert(31);
    println!("bitvec count {} / bit-set count {}", bv.count_ones(), bs.count());
}
