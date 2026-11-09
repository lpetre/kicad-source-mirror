//! Native counterpart of the wasm exports.
fn main() {
    let cases = std::fs::read_to_string("data/delaunay_cases.txt").unwrap();
    println!("delaunay_checksum     = {}", spatial_research::checksum::delaunay_checksum(&cases));
    println!(
        "packed_rtree_checksum = {}",
        spatial_research::checksum::packed_rtree_checksum(0x5254_5245_4500, 5000)
    );
    println!("nanoflann_checksum    = {}", spatial_research::checksum::nanoflann_checksum(0x4e41_4e4f));
    println!(
        "dynamic_rtree_checksum= {}",
        spatial_research::checksum::dynamic_rtree_checksum(0x5254_5245_4500, 5000)
    );
}
