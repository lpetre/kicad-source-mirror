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

//! Differential test against the C++ implementation.
//!
//! `tests/oracle/core_oracle.cpp` is a small C++ program that is compiled against the real
//! KiCad sources (`common/lset.cpp`, `layer_id.cpp`, `lseq.cpp`, `kiid.cpp`, `lib_id.cpp`, ...)
//! and prints a deterministic dump of what those functions return for a large number of
//! inputs: `tests/golden/core_oracle_dump.txt`.  This test produces the same dump with the
//! Rust port and compares the two line by line.  The structure of [`dump`] mirrors `main()`
//! of the C++ program; keep the two in sync.

use std::fmt::Write as _;

use kicad_core::base_set::BaseSet;
use kicad_core::kiid::{Kiid, KiidGenerator, KiidNilSetReset, KiidPath, NILUUID, nil_uuid};
use kicad_core::layer_ids::*;
use kicad_core::layer_range::LayerRange;
use kicad_core::lib_id::{LibId, get_lib_filename_forbidden_chars, get_lib_id_forbidden_chars};
use kicad_core::lseq::Lseq;
use kicad_core::lset::Lset;

use PcbLayerId as L;

// ---- shared helpers (mirrored in C++) ----

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;

fn fnv1a(mut h: u64, s: &[u8]) -> u64 {
    for &c in s {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x100000001b3);
    }

    h
}

/// The dump under construction.
struct Dump {
    /// The lines as they appear in the golden file.
    lines: Vec<String>,
    /// The same lines before shortening, for failure messages.
    full: Vec<String>,
    folding: bool,
    fold_hash: u64,
}

impl Dump {
    /// Mirror of the C++ `out()`: add one line.  Control characters are escaped, and lines
    /// longer than 120 bytes are shortened to a prefix followed by
    /// `...#<length>:<fnv1a hash of the whole line>`.
    fn out(&mut self, line: impl AsRef<str>) {
        let mut escaped = String::new();

        for c in line.as_ref().chars() {
            if (c as u32) < 0x20 {
                write!(escaped, "\\x{:02x}", c as u32).unwrap();
            } else {
                escaped.push(c);
            }
        }

        let mut line = escaped.clone();

        if line.len() > 120 {
            let tail = format!(
                "...#{}:{:016x}",
                line.len(),
                fnv1a(FNV_OFFSET, line.as_bytes())
            );

            // keep at most 32 leading bytes, stopping before the first non-ASCII byte
            let keep = line.bytes().take(32).take_while(|b| b.is_ascii()).count();

            line.truncate(keep);
            line += &tail;
        }

        if self.folding {
            // fold the line into a running hash instead of printing it
            self.fold_hash = fnv1a(self.fold_hash, line.as_bytes());
            self.fold_hash = fnv1a(self.fold_hash, b"\n");
            return;
        }

        self.lines.push(line);
        self.full.push(escaped);
    }

    fn begin_fold(&mut self) {
        self.folding = true;
        self.fold_hash = FNV_OFFSET;
    }

    fn end_fold(&mut self, tag: &str) {
        self.folding = false;
        self.out(format!("{tag} folded={:016x}", self.fold_hash));
    }
}

fn seq_str(seq: &[PcbLayerId]) -> String {
    if seq.is_empty() {
        return "-".to_string();
    }

    seq.iter()
        .map(|l| l.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn bits(values: &[bool]) -> String {
    values.iter().map(|&v| if v { '1' } else { '0' }).collect()
}

fn b(value: bool) -> i32 {
    i32::from(value)
}

fn layer_line(id: i32) -> String {
    format!(
        "{} {} {} {}",
        id,
        bits(&[
            is_valid_layer(id),
            is_pcb_layer(id),
            is_copper_layer(id),
            is_external_copper_layer(id),
            is_inner_copper_layer(id),
            is_non_copper_layer(id),
            is_copper_layer_incl_synthetic(id, true),
            is_copper_layer_incl_synthetic(id, false),
            is_solder_mask_layer(id),
            is_via_pad_layer(id),
            is_hole_layer(id),
            is_netname_layer(id),
            is_zone_fill_layer(id),
            is_pad_copper_layer(id),
            is_via_copper_layer(id),
            is_clearance_layer(id),
            is_drill_symbol_layer(id),
            is_points_layer(id),
            is_dcode_layer(id),
        ]),
        get_netname_layer(id),
        layer_name(id)
    )
}

fn random_lset(rng: &mut Lcg, density: u64) -> Lset {
    let mut set = Lset::new();

    for i in 0..PCB_LAYER_ID_COUNT {
        if rng.next() % 100 < density {
            set.set(i);
        }
    }

    set
}

fn dump_kiid(d: &mut Dump, tag: &str, id: &Kiid) {
    d.out(format!(
        "{} {} hash={} legacy={} ts={} {}",
        tag,
        id.as_std_string(),
        id.hash_value(),
        b(id.is_legacy_timestamp()),
        id.as_legacy_timestamp(),
        id.as_legacy_timestamp_string()
    ));
}

fn dump_lib_id(d: &mut Dump, tag: &str, ret: i32, id: &LibId) {
    d.out(format!(
        "{} ret={} nick=[{}] item=[{}] fmt=[{}] valid={} legacy={} empty={} full=[{}]",
        tag,
        ret,
        id.lib_nickname(),
        id.lib_item_name(),
        id.format(),
        b(id.is_valid()),
        b(id.is_legacy()),
        b(id.is_empty()),
        id.full_library_name()
    ));
}

/// Mirror of `main()` of `tests/oracle/core_oracle.cpp`.
fn dump() -> Dump {
    let mut d = Dump {
        lines: Vec::new(),
        full: Vec::new(),
        folding: false,
        fold_hash: 0,
    };

    // ================= A. layer ids =================
    for id in -3..=140 {
        d.out(format!("LN {}", layer_line(id)));
    }

    let mut start = 141;

    while start <= LAYER_ID_COUNT + 2 {
        let mut h = FNV_OFFSET;
        let end = (start + 63).min(LAYER_ID_COUNT + 2);

        for id in start..=end {
            h = fnv1a(h, layer_line(id).as_bytes());
            h = fnv1a(h, b"\n");
        }

        d.out(format!("LNH {start}-{end} {h:016x}"));
        start += 64;
    }

    let flip_counts = [0, 2, 3, 4, 5, 6, 8, 16, 32];

    for id in -3..=140 {
        let layer = PcbLayerId(id);
        let mut flips = String::new();

        for n in flip_counts {
            write!(flips, " {}", flip_layer(layer, n).0).unwrap();
        }

        d.out(format!(
            "LP {} {} {} ord={} name={} flip={}",
            id,
            bits(&[
                is_user_layer(layer),
                is_front_layer(layer),
                is_back_layer(layer)
            ]),
            bits(&[
                is_copper_layer_lower_than(layer, L::FCu),
                is_copper_layer_lower_than(layer, L::BCu),
                is_copper_layer_lower_than(layer, L::In2Cu),
                is_copper_layer_lower_than(L::FCu, layer),
                is_copper_layer_lower_than(L::BCu, layer),
                is_copper_layer_lower_than(L::In2Cu, layer),
            ]),
            copper_layer_to_ordinal(layer),
            Lset::name(layer),
            flips
        ));
    }

    for id in -5..=70 {
        d.out(format!("LEG {} {}", id, board_layer_from_legacy_id(id).0));
    }

    d.out(format!(
        "TOLAYER {} {} {}",
        to_layer_id(i32::MAX).0,
        to_layer_id(5).0,
        to_layer_id(-1).0
    ));

    // ================= B. NameToLayer =================
    {
        let mut names: Vec<String> = Vec::new();

        for id in -2..PCB_LAYER_ID_COUNT + 4 {
            names.push(Lset::name(PcbLayerId(id)));
            names.push(layer_name(id));
        }

        for s in [
            "",
            "F.Cu ",
            " F.Cu",
            "f.cu",
            "User.",
            "User.0",
            "User.1",
            "User.01",
            "User. 7",
            "User.+3",
            "User.-3",
            "User.46",
            "User.1000",
            "User.99999999999999999999",
            "User.9223372036854775807",
            "User.4611686018427387905",
            "User.1x",
            "User.1 ",
            "In",
            "In1",
            "In12",
            "In123",
            "In1.Cu",
            "In01.Cu",
            "In30.Cu",
            "In31.Cu",
            "In0.Cu",
            "In-1.Cu",
            "In1xyz",
            "In1.Cuu",
            "In 5.Cu",
            "Inner1.Cu",
            "In2147483648.Cu",
            "In99999999999999999999.Cu",
            "Internal",
            "Rescue",
            "rescue",
            "Margin",
            "Edge.Cuts",
            "B.CrtYd",
            "F.Courtyard",
            "User.Drawings",
            "Dwgs.User",
            "Cmts.User",
            "Eco1.User",
            "Eco2.User",
            "UNDEFINED",
            "undefined",
            "User.\u{e9}",
            "In\u{e9}\u{e9}\u{e9}",
            "In5\u{e9}\u{e9}\u{e9}",
            "In7ab",
        ] {
            names.push(s.to_string());
        }

        for n in &names {
            d.out(format!("N2L [{}] {}", n, Lset::name_to_layer(n)));
        }
    }

    // ================= C. LSET masks =================
    d.out(format!(
        "M InternalCuMask {}",
        Lset::internal_cu_mask().fmt_hex()
    ));
    d.out(format!(
        "M FrontAssembly {}",
        Lset::front_assembly().fmt_hex()
    ));
    d.out(format!(
        "M BackAssembly {}",
        Lset::back_assembly().fmt_hex()
    ));
    d.out(format!("M AllCuMask {}", Lset::all_cu_mask().fmt_hex()));
    d.out(format!(
        "M ExternalCuMask {}",
        Lset::external_cu_mask().fmt_hex()
    ));
    d.out(format!(
        "M AllNonCuMask {}",
        Lset::all_non_cu_mask().fmt_hex()
    ));
    d.out(format!(
        "M AllLayersMask {}",
        Lset::all_layers_mask().fmt_hex()
    ));
    d.out(format!(
        "M FrontTechMask {}",
        Lset::front_tech_mask().fmt_hex()
    ));
    d.out(format!(
        "M FrontBoardTechMask {}",
        Lset::front_board_tech_mask().fmt_hex()
    ));
    d.out(format!(
        "M BackTechMask {}",
        Lset::back_tech_mask().fmt_hex()
    ));
    d.out(format!(
        "M BackBoardTechMask {}",
        Lset::back_board_tech_mask().fmt_hex()
    ));
    d.out(format!("M AllTechMask {}", Lset::all_tech_mask().fmt_hex()));
    d.out(format!(
        "M AllBoardTechMask {}",
        Lset::all_board_tech_mask().fmt_hex()
    ));
    d.out(format!("M FrontMask {}", Lset::front_mask().fmt_hex()));
    d.out(format!("M BackMask {}", Lset::back_mask().fmt_hex()));
    d.out(format!(
        "M SideSpecificMask {}",
        Lset::side_specific_mask().fmt_hex()
    ));
    d.out(format!("M UserMask {}", Lset::user_mask().fmt_hex()));
    d.out(format!(
        "M PhysicalLayersMask {}",
        Lset::physical_layers_mask().fmt_hex()
    ));

    for n in -1..=64 {
        let m = Lset::all_cu_mask_n(n);
        d.out(format!("MCU {} {} {}", n, m.size(), m.fmt_hex()));
    }

    for n in -1..=50 {
        let m = Lset::user_defined_layers_mask(n);
        d.out(format!("MUD {} {} {}", n, m.size(), m.fmt_hex()));
    }

    // ================= D. LSET operations =================
    {
        let mut rng = Lcg(20240819);
        let mut sets: Vec<Lset> = vec![
            Lset::new(),
            Lset::all_layers_mask().clone(),
            Lset::all_cu_mask().clone(),
            Lset::all_non_cu_mask(),
            Lset::from([L::FCu]),
            Lset::from([L::BCu]),
            Lset::from([L::In7Cu]),
            Lset::from([L::User45]),
            Lset::from([
                L::FCu,
                L::BCu,
                L::FMask,
                L::BMask,
                L::FSilkS,
                L::BSilkS,
                L::EdgeCuts,
            ]),
        ];

        let densities = [3, 10, 25, 50, 75, 95];

        for k in 0..24 {
            sets.push(random_lset(&mut rng, densities[k % 6]));
        }

        let selected = [
            L::UndefinedLayer,
            L::FCu,
            L::In2Cu,
            L::FSilkS,
            L::User3,
            L::EdgeCuts,
            L::BFab,
        ];
        let flip_n = [0, 2, 3, 4, 6, 8, 16, 32];

        for k in 0..sets.len() {
            let s = &sets[k];
            let prev = &sets[(k + sets.len() - 1) % sets.len()];

            // Only the first sets are dumped in full, the others as one hash each.
            if k >= 20 {
                d.begin_fold();
            }

            d.out(format!(
                "S{} hex {} count={} extract={} any={} all={} hash={}",
                k,
                s.fmt_hex(),
                s.count(),
                s.extract_layer().0,
                b(s.any()),
                b(s.all()),
                s.hash_value()
            ));

            if k % 8 == 0 {
                d.out(format!("S{} bin {}", k, s.fmt_bin()));
            }

            d.out(format!("S{} seq {}", k, seq_str(&s.seq())));
            d.out(format!("S{} custack {}", k, seq_str(&s.cu_stack())));
            d.out(format!(
                "S{} techuser {}",
                k,
                seq_str(&s.tech_and_user_ui_order())
            ));
            d.out(format!("S{} uiorder {}", k, seq_str(&s.ui_order())));
            d.out(format!(
                "S{} top2bottom {}",
                k,
                seq_str(&s.seq_stackup_top2bottom(selected[k % 7]))
            ));
            d.out(format!(
                "S{} plotting {}",
                k,
                seq_str(&s.seq_stackup_for_plotting())
            ));

            {
                let it: Vec<PcbLayerId> = s.iter().collect();
                let mut run: Vec<PcbLayerId> = Vec::new();
                s.run_on_layers(|l| run.push(l));
                let nc: Vec<PcbLayerId> = s.non_copper_layers().collect();

                d.out(format!(
                    "S{} iter {} {} noncu {}",
                    k,
                    b(it == s.seq().0),
                    b(run == s.seq().0),
                    seq_str(&nc)
                ));
            }

            for n in flip_n {
                let mut f = s.clone();
                f.flip_standard_layers(n);
                d.out(format!("S{} flip{} {} {}", k, n, f.size(), f.fmt_hex()));
            }

            {
                let (mut a, mut bb, mut c) = (s.clone(), s.clone(), s.clone());
                a.clear_copper_layers();
                bb.clear_non_copper_layers();
                c.clear_user_defined_layers();
                d.out(format!(
                    "S{} clear {} {} {}",
                    k,
                    a.fmt_hex(),
                    bb.fmt_hex(),
                    c.fmt_hex()
                ));
            }

            {
                let a = s & prev;
                let o = s | prev;
                let x = s ^ prev;
                let n = !s;

                d.out(format!(
                    "S{} ops {} {} {} {} cmp={} lt={} eq={} containsall={}{}",
                    k,
                    a.fmt_hex(),
                    o.fmt_hex(),
                    x.fmt_hex(),
                    n.fmt_hex(),
                    s.compare(prev),
                    b(s < prev),
                    b(s == prev),
                    b(s.contains_all(prev)),
                    b(prev.contains_all(s))
                ));
            }

            {
                let c: String = [
                    L::FCu,
                    L::BCu,
                    L::In1Cu,
                    L::In30Cu,
                    L::FMask,
                    L::EdgeCuts,
                    L::User1,
                    L::User45,
                    L::UndefinedLayer,
                    L::UnselectedLayer,
                ]
                .iter()
                .map(|&l| if s.contains(l) { '1' } else { '0' })
                .collect();

                d.out(format!("S{k} contains {c}"));
            }

            {
                // FmtHex / ParseHex round trip
                let mut p = Lset::new();
                let consumed = p.parse_hex(&s.fmt_hex());
                d.out(format!("S{} parsehex {} {}", k, consumed, b(&p == s)));
            }

            if k >= 20 {
                d.end_fold(&format!("S{k}"));
            }
        }

        // Sets of other sizes
        let sizes = [1usize, 7, 10, 60, 64, 65, 100, 128, 129, 200];
        let mut bsets: Vec<BaseSet> = Vec::new();

        for sz in sizes {
            for _rep in 0..3 {
                let mut bs = BaseSet::new(sz);

                for i in 0..sz {
                    if rng.next() % 3 == 0 {
                        bs.set(i);
                    }
                }

                bsets.push(bs);
            }
        }

        for k in 0..bsets.len() {
            let s = &bsets[k];
            let prev = &bsets[(k + bsets.len() - 1) % bsets.len()];
            let far = &bsets[(k + 7) % bsets.len()];
            let a = s & far;
            let o = s | far;
            let x = s ^ far;
            let n = !s;

            d.out(format!(
                "B{} size={} hex {} bin {} count={} hash={} cmp={},{} lt={},{} eq={}",
                k,
                s.size(),
                s.fmt_hex(),
                s.fmt_bin(),
                s.count(),
                s.hash_value(),
                s.compare(prev),
                s.compare(far),
                b(s < prev),
                b(s < far),
                b(s == prev)
            ));
            d.out(format!(
                "B{} ops {} {} {} {} {} {} {} {}",
                k,
                a.size(),
                a.fmt_hex(),
                o.size(),
                o.fmt_hex(),
                x.size(),
                x.fmt_hex(),
                n.size(),
                n.fmt_hex()
            ));

            let as_lset = Lset::from_base_set(s.clone());
            // The copper layer iterator of LSET never terminates for a set of odd size.
            let even = s.size() % 2 == 0;

            d.out(format!(
                "B{} lset custack {} noncu-ui {} top2bottom {}",
                k,
                if even {
                    seq_str(&as_lset.cu_stack())
                } else {
                    "skipped".to_string()
                },
                seq_str(&as_lset.tech_and_user_ui_order()),
                if even {
                    seq_str(&as_lset.seq_stackup_top2bottom(L::UndefinedLayer))
                } else {
                    "skipped".to_string()
                }
            ));
        }

        // ParseHex of assorted strings into sets of several sizes
        for s in [
            "",
            "0",
            "1",
            "f",
            "ff",
            "FF",
            "aBcDeF",
            "0x1f",
            "1_f",
            "_",
            "__1",
            "zz",
            "1z2",
            "00010fc_ffffffff",
            "ffffffff_ffffffff_ffffffff_ffffffff",
            "1_00000000_00000000_00000000_00000000",
            "fffffffffffffffffffffffffffffffffff",
            "12 34",
            "-1",
            "0000000_00000000_55555555_5755f5ff",
            "8",
            "g",
            "1g",
        ] {
            for sz in [4usize, 10, 60, 64, 128] {
                let mut bs = BaseSet::new(sz);
                bs.set(1);
                let consumed = bs.parse_hex(s);
                d.out(format!("PH [{}] {} {} {}", s, sz, consumed, bs.fmt_hex()));
            }
        }
    }

    // ================= E. LayerCount / IsBetween =================
    {
        let cu = [
            L::FCu,
            L::BCu,
            L::In1Cu,
            L::In2Cu,
            L::In3Cu,
            L::In5Cu,
            L::In30Cu,
        ];
        let probe = [
            L::FCu,
            L::BCu,
            L::In1Cu,
            L::In2Cu,
            L::In3Cu,
            L::In4Cu,
            L::In5Cu,
            L::In29Cu,
            L::In30Cu,
            L::FMask,
            L::BMask,
            L::User1,
            L::UndefinedLayer,
        ];

        for a in cu {
            for bb in cu {
                let mut counts = String::new();

                for n in [2, 4, 6, 8, 32] {
                    write!(counts, " {}", Lset::layer_count(a, bb, n)).unwrap();
                }

                let between: String = probe
                    .iter()
                    .map(|&p| if Lset::is_between(a, bb, p) { '1' } else { '0' })
                    .collect();

                d.out(format!("LC {} {}{} between={}", a.0, bb.0, counts, between));
            }
        }

        d.out(format!(
            "LC noncu {} {} {}",
            Lset::layer_count(L::FMask, L::BCu, 6),
            Lset::layer_count(L::FCu, L::BMask, 4),
            Lset::layer_count(L::UndefinedLayer, L::FCu, 2)
        ));
        d.out(format!(
            "IB noncu {} {} {} {}",
            b(Lset::is_between(L::FMask, L::BCu, L::FCu)),
            b(Lset::is_between(L::FMask, L::BMask, L::BCu)),
            b(Lset::is_between(L::FCu, L::User1, L::In5Cu)),
            b(Lset::is_between(L::FMask, L::User1, L::FSilkS))
        ));
    }

    // ================= F. LAYER_RANGE =================
    for n in [2, 3, 4, 5, 6, 8, 32] {
        let mut stack = vec![L::FCu];

        for i in 1..=n - 2 {
            if i <= 3 || i >= n - 3 {
                stack.push(PcbLayerId(L::In1Cu.0 + (i - 1) * 2));
            }
        }

        stack.push(L::BCu);

        for &a in &stack {
            for &bb in &stack {
                let range = LayerRange::new(a, bb, n).unwrap();
                let layers: Vec<PcbLayerId> = range.iter().collect();
                let contains: String = stack
                    .iter()
                    .map(|&p| if range.contains(p) { '1' } else { '0' })
                    .collect();
                let from_range = Lset::from_layer_range(&range);

                d.out(format!(
                    "LR {} {} {} size={} [{}] contains={} lset={}:{}",
                    n,
                    a.0,
                    bb.0,
                    range.size(),
                    seq_str(&layers),
                    contains,
                    from_range.size(),
                    from_range.fmt_hex()
                ));
            }
        }
    }

    for n in [0, 1, -7, 64, 65, 128] {
        let range = LayerRange::new(L::FCu, L::BCu, n).unwrap();
        let layers: Vec<PcbLayerId> = range.iter().collect();

        d.out(format!(
            "LRN {} size={} count={} last={}",
            n,
            range.size(),
            layers.len(),
            layers[layers.len() - 2].0
        ));
    }

    for a in [0, 2, 4, 6, 60] {
        for bb in [0, 2, 4, 6, 60] {
            let c: String = [0, 2, 4, 6, 8, 60, 62, 1, -1]
                .iter()
                .map(|&t| {
                    if LayerRange::contains_range(a, bb, t) {
                        '1'
                    } else {
                        '0'
                    }
                })
                .collect();

            d.out(format!("LRC {a} {bb} {c}"));
        }
    }

    {
        let seq: Lseq = Lset::all_layers_mask().seq_stackup_top2bottom(L::UndefinedLayer);

        d.out(format!(
            "TL {} {} {} {} {}",
            seq.test_layers(L::FCu, L::FCu),
            seq.test_layers(L::FCu, L::In1Cu),
            seq.test_layers(L::In1Cu, L::FCu),
            seq.test_layers(L::BFab, L::EdgeCuts),
            seq.test_layers(L::UndefinedLayer, L::FCu)
        ));
    }

    // ================= G. KIID =================
    // The C++ generator state is global; here it is this explicit generator.
    let mut generator = KiidGenerator::default();

    for seed in [0u32, 1, 42, 123456789, 4294967295] {
        generator.seed_generator(seed);

        for _ in 0..6 {
            let id = Kiid::new(&mut generator);
            dump_kiid(&mut d, "KS", &id);
        }
    }

    generator.seed_generator(7);
    let mut parsed: Vec<Kiid> = Vec::new();

    for s in [
        "0",
        "1",
        "a",
        "F",
        "12",
        "123",
        "1234",
        "12345",
        "123456",
        "1234567",
        "12345678",
        "AABBCCDD",
        "aabbccdd",
        "00000012",
        "123456789",
        "1234567g",
        "g",
        "",
        " ",
        "0x12",
        "01234567-89ab-cdef-0123-456789abcdef",
        "0123456789abcdef0123456789abcdef",
        "{01234567-89ab-cdef-0123-456789abcdef}",
        "{0123456789abcdef0123456789abcdef}",
        "01234567-89abcdef0123456789abcdef",
        "0123456789ab-cdef-0123-456789abcdef",
        "01234567-89ab-cdef-0123-456789abcdef0",
        "01234567-89ab-cdef-0123-456789abcde",
        "{01234567-89ab-cdef-0123-456789abcdef",
        "01234567-89ab-cdef-0123-456789abcdef}",
        "01234567-89AB-CDEF-0123-456789ABCDEF",
        "g1234567-89ab-cdef-0123-456789abcdef",
        "01234567-89ab-cdef-0123456789abcdef",
        " 01234567-89ab-cdef-0123-456789abcdef",
        "00000000-0000-0000-0000-000000000000",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "00000000-0000-0000-0000-00005f0b8a2c",
        "5f0b8a2c-0000-0000-0000-000000000000",
        "r\u{e9}sistor",
        "\u{e9}",
        "deadbeef",
        "DEADBEEF1",
    ] {
        let id = Kiid::from_string_or_random(s, &mut generator);
        parsed.push(id);
        d.out(format!("KP [{}] sniff={}", s, b(Kiid::sniff_test(s))));
        dump_kiid(&mut d, "KP  ", &id);
    }

    {
        generator.create_nil_uuids(true);
        let nil1 = Kiid::new(&mut generator);
        let bad = Kiid::from_string_or_random("not-a-uuid", &mut generator);
        // NB: KIID( timestamp_t ) leaves the first 12 bytes uninitialised with Boost < 1.86, so
        // the legacy timestamp is built from its string form here.
        let mut ts = Kiid::from_string_or_random("5F0B8A2C", &mut generator);
        dump_kiid(&mut d, "KN nil", &nil1);
        dump_kiid(&mut d, "KN bad", &bad);
        dump_kiid(&mut d, "KN ts", &ts);
        ts.convert_timestamp_to_uuid(&mut generator);
        dump_kiid(&mut d, "KN converted", &ts);
        let keep = ts;
        ts.convert_timestamp_to_uuid(&mut generator);
        d.out(format!("KN unchanged {}", b(keep == ts)));
        generator.create_nil_uuids(false);
        let after = Kiid::new(&mut generator);
        dump_kiid(&mut d, "KN after", &after);

        {
            let mut guard = KiidNilSetReset::new(&mut generator);
            let guarded = Kiid::new(&mut guard);
            dump_kiid(&mut d, "KN guarded", &guarded);
        }

        let after2 = Kiid::new(&mut generator);
        dump_kiid(&mut d, "KN after2", &after2);
        d.out(format!(
            "KN niluuid {} {}",
            NILUUID.as_std_string(),
            b(nil_uuid() == NILUUID)
        ));
    }

    for pair in parsed.chunks_exact(2) {
        let c = Kiid::combine(&pair[0], &pair[1]);
        let mut inc = pair[0];
        inc.increment();

        d.out(format!(
            "KC {} {} lt={} gt={} eq={}",
            c.as_std_string(),
            inc.as_std_string(),
            b(pair[0] < pair[1]),
            b(pair[0] > pair[1]),
            b(pair[0] == pair[1])
        ));
    }

    {
        let mut sorted = parsed.clone();
        sorted.sort();
        let mut h = FNV_OFFSET;

        for id in &sorted {
            h = fnv1a(h, id.as_std_string().as_bytes());
            h = fnv1a(h, b"\n");
        }

        d.out(format!(
            "KSORT {} {} {:016x}",
            sorted.first().unwrap().as_std_string(),
            sorted.last().unwrap().as_std_string(),
            h
        ));
    }

    for s in [
        "",
        "a",
        "Device:R",
        "R_0402_1005Metric",
        "some known string",
        "R\u{e9}sistance",
        "\u{65e5}\u{672c}\u{8a9e}",
        "\u{1f600} smile",
        "a longer name that spans more than one sha1 block: 0123456789012345678901234567890123456789",
        "exactly-fifty-five-bytes-of-payload-with-namespace-uuid",
    ] {
        d.out(format!(
            "KD [{}] {} {}",
            s,
            Kiid::from_deterministic_string(s).as_std_string(),
            Kiid::from_name(s).as_std_string()
        ));
    }

    {
        let mut bytes = [0u8; 16];

        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = (i * 17 + 3) as u8;
        }

        let fb = Kiid::from_bytes(bytes);
        let mut inc = fb;

        for _ in 0..300 {
            inc.increment();
        }

        let mut wrap = Kiid::parse("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
        wrap.increment();

        d.out(format!(
            "KB {} {} {} {}",
            fb.as_std_string(),
            inc.as_std_string(),
            wrap.as_std_string(),
            b(fb.as_bytes() == bytes)
        ));
    }

    // KIID_PATH
    generator.seed_generator(99);

    {
        let mut paths: Vec<KiidPath> = Vec::new();

        for s in [
            "",
            "/",
            "//",
            "/00000000-0000-0000-0000-00005f0b8a2c",
            "/00000000-0000-0000-0000-00005f0b8a2c/00000000-0000-0000-0000-00005f0b8a2d",
            "00000000-0000-0000-0000-00005f0b8a2c/00000000-0000-0000-0000-00005f0b8a2d/",
            "/00000000-0000-0000-0000-00005f0b8a2c/00000000-0000-0000-0000-00005f0b8a2d/\
             e63e39d7-6ac0-4ffd-8aa3-1841a4541b55",
            "/5F0B8A2C/5F0B8A2D",
            "/garbage/5F0B8A2D",
            "/a\\/b/c",
            "/00000000-0000-0000-0000-00005f0b8a2d",
            "/e63e39d7-6ac0-4ffd-8aa3-1841a4541b55",
            "/00000000-0000-0000-0000-00005f0b8a2d/e63e39d7-6ac0-4ffd-8aa3-1841a4541b55",
        ] {
            let p = KiidPath::from_string_or_random(s, &mut generator);

            d.out(format!(
                "PP [{}] n={} [{}] hash={}",
                s,
                p.len(),
                p.as_string(),
                p.hash_value()
            ));
            paths.push(p);
        }

        for i in 0..paths.len() {
            let mut row = String::new();

            for j in 0..paths.len() {
                let mut rel = paths[i].clone();
                let ok = rel.make_relative_to(&paths[j]);

                row.push(' ');
                row += &bits(&[
                    paths[i] == paths[j],
                    paths[i] < paths[j],
                    paths[i] > paths[j],
                    paths[i].ends_with(&paths[j]),
                    paths[i].is_contained_within(&paths[j]),
                    ok,
                ]);
                write!(row, ":{}", rel.len()).unwrap();
            }

            let sum = paths[i].clone() + &paths[(i + 1) % paths.len()];
            d.out(format!("PR {}{} sum={}", i, row, sum.len()));
        }
    }

    // ================= H. LIB_ID =================
    {
        let mut ids: Vec<LibId> = Vec::new();

        for s in [
            "",
            ":",
            "a",
            "a:",
            ":a",
            "Package_SO:DGG56",
            "DGG56",
            "lib:item:extra",
            "lib::item",
            "lib:it/em",
            "lib:it em",
            "lib name:item",
            "lib:item<",
            "lib:>item",
            "lib:it\"em",
            "lib:it\\em",
            "lib:it\tem",
            "lib:it\nem",
            "lib:it\rem",
            "li\\b:item",
            "li\tb:item",
            "lib:%$/",
            "Biblioth\u{e8}que:R\u{e9}sistance",
            "\u{65e5}:\u{672c}<\u{8a9e}",
            "lib:item ",
            " lib : item ",
            "LIB:ITEM",
            "lib:Item",
            "liB:item",
            "::",
            "a:b:c:d<e",
            "<>\"\\:\t\n\r",
            "x<>\"\\\t\n\r",
        ] {
            let mut id = LibId::new();
            let mut ret = id.parse(s, false);
            d.out(format!("LI [{s}]"));
            dump_lib_id(&mut d, "LI   parse", ret, &id);
            ids.push(id);

            let mut fixed = LibId::new();
            ret = fixed.parse(s, true);
            dump_lib_id(&mut d, "LI   fixed", ret, &fixed);

            d.out(format!(
                "LI   illegal={} fixlib=[{}] fixitem=[{}] badlibchar={}",
                LibId::has_illegal_chars(s),
                LibId::fix_illegal_chars(s, true),
                LibId::fix_illegal_chars(s, false),
                LibId::find_illegal_library_name_char(s)
            ));

            match LibId::format_parts(s, "item") {
                Ok(f) => d.out(format!("LI   format=[{f}]")),
                Err(e) => d.out(format!("LI   format threw [{e}]")),
            }

            let mut direct = LibId::from_parts(s, "it:em");
            direct.set_sub_library_name("sub");
            let nick_ret = direct.set_lib_nickname("ni:ck");
            dump_lib_id(&mut d, "LI   direct", nick_ret, &direct);
        }

        for i in 0..ids.len() {
            let row: String = ids
                .iter()
                .map(|other| match ids[i].compare(other) {
                    c if c < 0 => '<',
                    c if c > 0 => '>',
                    _ => '=',
                })
                .collect();

            d.out(format!("LIC {i} {row}"));
        }

        d.out(format!(
            "LIF [{}] [{}]",
            get_lib_id_forbidden_chars(),
            get_lib_filename_forbidden_chars()
        ));
    }

    d
}

#[test]
fn rust_port_matches_cpp_dump() {
    let golden = include_str!("golden/core_oracle_dump.txt");
    let expected: Vec<&str> = golden.lines().collect();
    let actual = dump();

    let mut mismatches = Vec::new();

    for (i, line) in actual.lines.iter().enumerate() {
        match expected.get(i) {
            Some(exp) if exp == line => {}
            Some(exp) => mismatches.push(format!(
                "line {}:\n  C++ : {}\n  Rust: {}\n  full: {}",
                i + 1,
                exp,
                line,
                actual.full[i]
            )),
            None => mismatches.push(format!("line {}: missing in the C++ dump: {}", i + 1, line)),
        }

        if mismatches.len() >= 20 {
            break;
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} or more lines differ from the C++ dump:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(actual.lines.len(), expected.len(), "number of dump lines");
}
