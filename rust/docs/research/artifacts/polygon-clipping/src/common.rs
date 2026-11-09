// Shared case-file reader / output helpers for the differential drivers (scratch research code).
#![allow(dead_code)]

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};

pub type P3 = (i64, i64, i64);

#[derive(Default, Clone)]
pub struct Case {
    pub id: i64,
    pub cat: String,
    pub op: String,
    pub args: Vec<String>,
    pub subj: Vec<Vec<P3>>,
    pub clip: Vec<Vec<P3>>,
    pub groups: Vec<Vec<Vec<P3>>>,
    pub npoints: i64,
}

impl Case {
    pub fn arg_i64(&self, i: usize) -> i64 {
        self.args[i].parse().unwrap()
    }
    pub fn arg_f64(&self, i: usize) -> f64 {
        f64::from_bits(self.args[i].parse::<u64>().unwrap())
    }
}

fn parse_path<'a>(it: &mut impl Iterator<Item = &'a str>) -> Vec<P3> {
    let n: usize = it.next().unwrap().parse().unwrap();
    let mut p = Vec::with_capacity(n);
    for _ in 0..n {
        let x: i64 = it.next().unwrap().parse().unwrap();
        let y: i64 = it.next().unwrap().parse().unwrap();
        let z: i64 = it.next().unwrap().parse().unwrap();
        p.push((x, y, z));
    }
    p
}

/// In-memory variant of [`drive`] (used by the wasm build): no panic catching.
pub fn run_text(input: &str, run: impl Fn(&Case, &mut String)) -> String {
    let mut output = String::new();
    let mut cur = Case::default();
    for line in input.lines() {
        let mut it = line.split_ascii_whitespace();
        let Some(tag) = it.next() else { continue };
        match tag {
            "CASE" => {
                cur = Case::default();
                cur.id = it.next().unwrap().parse().unwrap();
                cur.cat = it.next().unwrap().to_string();
                cur.op = it.next().unwrap().to_string();
                cur.args = it.map(|s| s.to_string()).collect();
            }
            "G" => cur.groups.push(Vec::new()),
            "S" => {
                let p = parse_path(&mut it);
                cur.npoints += p.len() as i64;
                if let Some(g) = cur.groups.last_mut() {
                    g.push(p.clone());
                }
                cur.subj.push(p);
            }
            "C" => {
                let p = parse_path(&mut it);
                cur.npoints += p.len() as i64;
                cur.clip.push(p);
            }
            "END" => {
                writeln!(output, "CASE {}", cur.id).unwrap();
                run(&cur, &mut output);
                output.push_str("END\n");
            }
            _ => {}
        }
    }
    output
}

/// Reads the case file and calls `run` for every case; writes `CASE id` / result / `END`.
pub fn drive(run: impl Fn(&Case, &mut String)) {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <cases> <out>", args[0]);
        std::process::exit(2);
    }
    let input = BufReader::new(std::fs::File::open(&args[1]).unwrap());
    let mut output = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    // Keep panics quiet: they are reported per case.
    std::panic::set_hook(Box::new(|_| {}));
    let mut cur = Case::default();
    for line in input.lines() {
        let line = line.unwrap();
        let mut it = line.split_ascii_whitespace();
        let Some(tag) = it.next() else { continue };
        match tag {
            "CASE" => {
                cur = Case::default();
                cur.id = it.next().unwrap().parse().unwrap();
                cur.cat = it.next().unwrap().to_string();
                cur.op = it.next().unwrap().to_string();
                cur.args = it.map(|s| s.to_string()).collect();
            }
            "G" => cur.groups.push(Vec::new()),
            "S" => {
                let p = parse_path(&mut it);
                cur.npoints += p.len() as i64;
                if let Some(g) = cur.groups.last_mut() {
                    g.push(p.clone());
                }
                cur.subj.push(p);
            }
            "C" => {
                let p = parse_path(&mut it);
                cur.npoints += p.len() as i64;
                cur.clip.push(p);
            }
            "END" => {
                let mut out = String::new();
                let case = cur.clone();
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut s = String::new();
                    run(&case, &mut s);
                    s
                }));
                writeln!(out, "CASE {}", cur.id).unwrap();
                match res {
                    Ok(s) => out.push_str(&s),
                    Err(e) => {
                        let msg = e
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_default();
                        writeln!(out, "PANIC {}", msg.replace('\n', " ")).unwrap();
                    }
                }
                out.push_str("END\n");
                output.write_all(out.as_bytes()).unwrap();
            }
            _ => {}
        }
    }
    output.flush().unwrap();
}

pub fn put_path(out: &mut String, tag: &str, pts: impl Iterator<Item = P3>, n: usize) {
    out.push_str(tag);
    write!(out, " {}", n).unwrap();
    for (x, y, z) in pts {
        write!(out, " {} {} {}", x, y, z).unwrap();
    }
    out.push('\n');
}
