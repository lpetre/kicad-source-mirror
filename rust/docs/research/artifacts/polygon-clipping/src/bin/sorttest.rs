// Checks the Rust port of libstdc++ std::sort against the real thing (see cpp/sorttest.cpp).
// Input: lines "n k0 k1 ... " ; output: the tags (original indices) in sorted order.
use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let mut it = line.split_ascii_whitespace();
        let Some(n) = it.next() else { continue };
        let n: usize = n.parse().unwrap();
        let mut v: Vec<(i64, i64)> = (0..n).map(|i| (it.next().unwrap().parse().unwrap(), i as i64)).collect();
        clipper2_rs_gnusort::gnu_sort_pairs_for_test(&mut v);
        let s: Vec<String> = v.iter().map(|p| p.1.to_string()).collect();
        writeln!(out, "{}", s.join(" ")).unwrap();
    }
    eprintln!("heap fallbacks: {}", clipper2_rs_gnusort::gnu_sort_heap_fallbacks());
}
