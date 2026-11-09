//! Research tool: `sul::dynamic_bitset<uint64_t>` (base of KiCad's BASE_SET/LSET) vs
//! (a) a std-only `Vec<u64>` bit set and (b) the `fixedbitset` crate.
//!   bitset_cmp gen <script.txt>
//!   bitset_cmp compare <script.txt> <cpp.out>
use fixedbitset::FixedBitSet;
use spatial_research::rng::Rng;
use std::fmt::Write as _;

/// std-only dynamic bit set with `sul::dynamic_bitset<uint64_t>` semantics
/// (unused high bits of the last block are kept at zero).
#[derive(Clone, PartialEq, Eq, Default)]
struct StdBitSet {
    blocks: Vec<u64>,
    len: usize,
}

impl StdBitSet {
    fn new(len: usize) -> Self {
        StdBitSet { blocks: vec![0; len.div_ceil(64)], len }
    }
    fn sanitize(&mut self) {
        let extra = self.len % 64;
        if extra != 0 {
            let last = self.blocks.len() - 1;
            self.blocks[last] &= (1u64 << extra) - 1;
        }
    }
    fn resize(&mut self, len: usize) {
        self.blocks.resize(len.div_ceil(64), 0);
        self.len = len;
        self.sanitize();
    }
    fn set(&mut self, i: usize) {
        self.blocks[i / 64] |= 1 << (i % 64);
    }
    fn reset(&mut self, i: usize) {
        self.blocks[i / 64] &= !(1 << (i % 64));
    }
    fn flip(&mut self, i: usize) {
        self.blocks[i / 64] ^= 1 << (i % 64);
    }
    fn test(&self, i: usize) -> bool {
        self.blocks[i / 64] >> (i % 64) & 1 == 1
    }
    fn set_all(&mut self) {
        self.blocks.fill(u64::MAX);
        self.sanitize();
    }
    fn reset_all(&mut self) {
        self.blocks.fill(0);
    }
    fn flip_all(&mut self) {
        for b in &mut self.blocks {
            *b = !*b;
        }
        self.sanitize();
    }
    fn count(&self) -> usize {
        self.blocks.iter().map(|b| b.count_ones() as usize).sum()
    }
    fn any(&self) -> bool {
        self.blocks.iter().any(|&b| b != 0)
    }
    fn all(&self) -> bool {
        self.count() == self.len
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("gen") => {
            let mut rng = Rng::new(0xb175);
            let mut s = String::new();
            let mut size = 0usize;
            for step in 0..200_000 {
                if step % 400 == 0 {
                    size = [128usize, 128, 128, 64, 60, 65, 1, 200, 0, 127, 129][rng.range(0, 10) as usize];
                    writeln!(s, "N {size}").unwrap();
                    continue;
                }
                let bit = if size == 0 { 0 } else { rng.range(0, size as i64 - 1) as usize };
                let op = rng.range(0, 99);
                if size == 0 && op < 70 {
                    writeln!(s, "FA 0").unwrap();
                    continue;
                }
                let line = match op {
                    0..=24 => format!("S {bit}"),
                    25..=39 => format!("R {bit}"),
                    40..=49 => format!("F {bit}"),
                    50..=69 => format!("B {bit}"),
                    70..=72 => "SA 0".to_string(),
                    73..=75 => "RA 0".to_string(),
                    76..=79 => "FA 0".to_string(),
                    80..=84 => "AND 0".to_string(),
                    85..=89 => "OR 0".to_string(),
                    90..=93 => "XOR 0".to_string(),
                    94..=95 => "NOT 0".to_string(),
                    96..=97 => "BC 0".to_string(),
                    _ => {
                        size = rng.range(0, 260) as usize;
                        format!("RS {size}")
                    }
                };
                writeln!(s, "{line}").unwrap();
            }
            std::fs::write(&args[2], s).unwrap();
        }
        Some("compare") => {
            let script = std::fs::read_to_string(&args[2]).unwrap();
            let cpp = std::fs::read_to_string(&args[3]).unwrap();
            let (mut a, mut b) = (StdBitSet::default(), StdBitSet::default());
            let (mut fa, mut fb) = (FixedBitSet::new(), FixedBitSet::new());
            let (mut n, mut std_ok, mut fixed_ok) = (0usize, 0usize, 0usize);
            for (line, expect) in script.lines().zip(cpp.lines()) {
                let mut it = line.split(' ');
                let op = it.next().unwrap();
                let arg: usize = it.next().unwrap().parse().unwrap();
                match op {
                    "N" => {
                        a = StdBitSet::new(arg);
                        b = StdBitSet::new(arg);
                        fa = FixedBitSet::with_capacity(arg);
                        fb = FixedBitSet::with_capacity(arg);
                    }
                    "S" => {
                        a.set(arg);
                        fa.insert(arg);
                    }
                    "R" => {
                        a.reset(arg);
                        fa.remove(arg);
                    }
                    "F" => {
                        a.flip(arg);
                        fa.toggle(arg);
                    }
                    "SA" => {
                        a.set_all();
                        fa.insert_range(..);
                    }
                    "RA" => {
                        a.reset_all();
                        fa.clear();
                    }
                    "FA" | "NOT" => {
                        a.flip_all();
                        fa.toggle_range(..);
                    }
                    "RS" => {
                        a.resize(arg);
                        b.resize(arg);
                        // FixedBitSet::grow never shrinks: rebuild to get resize semantics
                        let resize = |f: &FixedBitSet| {
                            let mut g = FixedBitSet::with_capacity(arg);
                            for i in f.ones().filter(|&i| i < arg) {
                                g.insert(i);
                            }
                            g
                        };
                        fa = resize(&fa);
                        fb = resize(&fb);
                    }
                    "B" => {
                        b.set(arg);
                        fb.insert(arg);
                    }
                    "BC" => {
                        b.reset_all();
                        fb.clear();
                    }
                    "AND" => {
                        for (x, y) in a.blocks.iter_mut().zip(&b.blocks) {
                            *x &= *y;
                        }
                        fa.intersect_with(&fb);
                    }
                    "OR" => {
                        for (x, y) in a.blocks.iter_mut().zip(&b.blocks) {
                            *x |= *y;
                        }
                        fa.union_with(&fb);
                    }
                    "XOR" => {
                        for (x, y) in a.blocks.iter_mut().zip(&b.blocks) {
                            *x ^= *y;
                        }
                        fa.symmetric_difference_with(&fb);
                    }
                    _ => panic!(),
                }
                let bits: String = (0..a.len).map(|i| if a.test(i) { '1' } else { '0' }).collect();
                let mine = format!(
                    "{} {} {} {} {} {}",
                    a.count(),
                    a.any() as u8,
                    !a.any() as u8,
                    a.all() as u8,
                    (a == b) as u8,
                    bits
                );
                let fbits: String = (0..fa.len()).map(|i| if fa.contains(i) { '1' } else { '0' }).collect();
                let fixed = format!(
                    "{} {} {} {} {} {}",
                    fa.count_ones(..),
                    !fa.is_clear() as u8,
                    fa.is_clear() as u8,
                    fa.is_full() as u8,
                    (fa == fb) as u8,
                    fbits
                );
                n += 1;
                if mine == expect {
                    std_ok += 1;
                }
                if fixed == expect {
                    fixed_ok += 1;
                }
            }
            println!("{n} operations: std-only Vec<u64> bit set identical after {std_ok}; fixedbitset identical after {fixed_ok}");
        }
        _ => eprintln!("usage"),
    }
}
