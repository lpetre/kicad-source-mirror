//! Smoke test of rectangle-packing crates (alternatives to rectpack2d; out of port scope).
fn main() {
    // crunch
    let items: Vec<crunch::Item<usize>> =
        (0..20).map(|i| crunch::Item::new(i, 10 + i * 3, 8 + (i * 7) % 20, crunch::Rotation::None)).collect();
    match crunch::pack_into_po2(1024, items) {
        Ok(p) => println!("crunch 0.5.3: packed {} rects into {}x{}", p.items.len(), p.w, p.h),
        Err(_) => println!("crunch 0.5.3: failed"),
    }
    // guillotiere
    let mut atlas = guillotiere::AtlasAllocator::new(guillotiere::size2(256, 256));
    let n = (0..20).filter(|i| atlas.allocate(guillotiere::size2(10 + i * 3, 8 + (i * 7) % 20)).is_some()).count();
    println!("guillotiere 0.7.0: allocated {n} rects");
    // etagere
    let mut shelf = etagere::AtlasAllocator::new(etagere::size2(256, 256));
    let n = (0..20).filter(|i| shelf.allocate(etagere::size2(10 + i * 3, 8 + (i * 7) % 20)).is_some()).count();
    println!("etagere 0.3.0: allocated {n} rects");
    // rectangle-pack
    use rectangle_pack::*;
    use std::collections::BTreeMap;
    let mut rects: GroupedRectsToPlace<usize, ()> = GroupedRectsToPlace::new();
    for i in 0..20u32 {
        rects.push_rect(i as usize, None, RectToInsert::new(10 + i * 3, 8 + (i * 7) % 20, 1));
    }
    let mut bins = BTreeMap::new();
    bins.insert(0, TargetBin::new(256, 256, 1));
    match pack_rects(&rects, &mut bins, &volume_heuristic, &contains_smallest_box) {
        Ok(p) => println!("rectangle-pack 0.4.2: placed {} rects", p.packed_locations().len()),
        Err(e) => println!("rectangle-pack 0.4.2: {e:?}"),
    }
}
