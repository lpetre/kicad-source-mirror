//! Research tool: verify the libstdc++ `std::sort` / `std::partial_sort` port against g++.
//!   sort_cmp gen <cases.txt>
//!   sort_cmp compare <cases.txt> <cpp.out>
use spatial_research::rng::Rng;
use spatial_research::stdsort::{HEAPSORT_FALLBACKS, std_partial_sort, std_sort};
use std::cell::RefCell;
use std::fmt::Write as _;
use std::sync::atomic::Ordering;

struct Case {
    mode: char,
    m: usize,
    keys: Vec<i64>,
}

/// McIlroy's "A Killer Adversary for Quicksort": builds the worst-case input for the victim.
fn antiqsort(n: usize) -> Vec<i64> {
    let gas = n as i64;
    let val = RefCell::new(vec![gas; n]);
    let nsolid = RefCell::new(0i64);
    let candidate = RefCell::new(0usize);
    let mut ptr: Vec<usize> = (0..n).collect();
    std_sort(&mut ptr, |&x, &y| {
        let mut val = val.borrow_mut();
        if val[x] == gas && val[y] == gas {
            let mut ns = nsolid.borrow_mut();
            if x == *candidate.borrow() {
                val[x] = *ns;
            } else {
                val[y] = *ns;
            }
            *ns += 1;
        }
        if val[x] == gas {
            *candidate.borrow_mut() = x;
        } else if val[y] == gas {
            *candidate.borrow_mut() = y;
        }
        val[x] < val[y]
    });
    val.into_inner()
}

fn generate() -> Vec<Case> {
    let mut rng = Rng::new(0x534f_5254);
    let mut cases = Vec::new();
    // random keys with a controlled number of distinct values (ties)
    for _ in 0..3000 {
        let n = match rng.range(0, 9) {
            0 => rng.range(0, 3) as usize,
            1..=4 => rng.range(0, 40) as usize,
            5..=7 => rng.range(0, 400) as usize,
            _ => rng.range(0, 5000) as usize,
        };
        let distinct = [1i64, 2, 3, 5, 10, 100, 1_000_000_000][rng.range(0, 6) as usize];
        let keys: Vec<i64> = (0..n).map(|_| rng.range(0, distinct - 1)).collect();
        cases.push(Case { mode: 'S', m: 0, keys });
    }
    // structured inputs: sorted, reversed, organ pipe, sawtooth, mostly-equal
    for n in [0usize, 1, 2, 15, 16, 17, 18, 31, 32, 33, 64, 100, 1000, 4096, 10000] {
        let asc: Vec<i64> = (0..n as i64).collect();
        let desc: Vec<i64> = (0..n as i64).rev().collect();
        let organ: Vec<i64> = (0..n as i64).map(|i| i.min(n as i64 - 1 - i)).collect();
        let saw: Vec<i64> = (0..n as i64).map(|i| i % 7).collect();
        let pairs: Vec<i64> = (0..n as i64).map(|i| i / 2).collect();
        let mut one_off = vec![5i64; n];
        if n > 3 {
            one_off[n / 3] = 1;
            one_off[n - 1] = 9;
        }
        for keys in [asc, desc, organ, saw, pairs, one_off] {
            cases.push(Case { mode: 'S', m: 0, keys });
        }
    }
    // adversarial inputs that drive introsort into its heapsort fallback
    for n in [40usize, 100, 257, 1000, 3000, 10000] {
        let keys = antiqsort(n);
        cases.push(Case { mode: 'S', m: 0, keys: keys.clone() });
        // the same shape with ties
        cases.push(Case { mode: 'S', m: 0, keys: keys.iter().map(|k| k / 3).collect() });
    }
    // std::partial_sort (heap code paths)
    for _ in 0..1500 {
        let n = rng.range(0, 600) as usize;
        let m = rng.range(0, n as i64) as usize;
        let distinct = [1i64, 2, 5, 50, 1_000_000][rng.range(0, 4) as usize];
        let keys: Vec<i64> = (0..n).map(|_| rng.range(0, distinct - 1)).collect();
        cases.push(Case { mode: 'P', m, keys });
    }
    cases
}

fn run(c: &Case) -> Vec<usize> {
    let mut v: Vec<(i64, usize)> = c.keys.iter().copied().zip(0..).collect();
    if c.mode == 'S' {
        std_sort(&mut v, |a, b| a.0 < b.0);
    } else {
        std_partial_sort(&mut v, c.m, |a, b| a.0 < b.0);
    }
    v.into_iter().map(|p| p.1).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("gen") => {
            let cases = generate();
            let mut s = String::new();
            writeln!(s, "{}", cases.len()).unwrap();
            for c in &cases {
                write!(s, "{} {} {}", c.mode, c.keys.len(), c.m).unwrap();
                for k in &c.keys {
                    write!(s, " {k}").unwrap();
                }
                s.push('\n');
            }
            std::fs::write(&args[2], s).unwrap();
            println!("{} cases", cases.len());
        }
        Some("compare") => {
            let cases = generate();
            HEAPSORT_FALLBACKS.store(0, Ordering::Relaxed);
            let text = std::fs::read_to_string(&args[3]).unwrap();
            let (mut same, mut diff, mut sort_cases, mut psort_cases) = (0, 0, 0, 0);
            let mut rust_unstable_same = 0;
            let mut with_ties = 0;
            for (c, line) in cases.iter().zip(text.lines()) {
                let cpp: Vec<usize> = line.split_ascii_whitespace().map(|x| x.parse().unwrap()).collect();
                let r = run(c);
                if r == cpp {
                    same += 1;
                } else {
                    diff += 1;
                }
                if c.mode == 'S' {
                    sort_cases += 1;
                    let mut k = c.keys.clone();
                    k.sort();
                    let before = k.len();
                    k.dedup();
                    if k.len() != before {
                        with_ties += 1;
                    }
                    // what Rust's own unstable sort does with the same comparator
                    let mut v: Vec<(i64, usize)> = c.keys.iter().copied().zip(0..).collect();
                    v.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                    if v.into_iter().map(|p| p.1).collect::<Vec<_>>() == cpp {
                        rust_unstable_same += 1;
                    }
                } else {
                    psort_cases += 1;
                }
            }
            println!(
                "std_sort port vs libstdc++: {same} identical, {diff} different \
                 ({sort_cases} std::sort cases, {with_ties} with ties; {psort_cases} std::partial_sort cases)"
            );
            println!("heapsort fallback taken {} times in the port", HEAPSORT_FALLBACKS.load(Ordering::Relaxed));
            println!(
                "for comparison, slice::sort_unstable_by gives the libstdc++ order in {rust_unstable_same}/{sort_cases} std::sort cases"
            );
        }
        _ => eprintln!("usage"),
    }
}
