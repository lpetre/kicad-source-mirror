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

//! Differential test of `kicad_core::eda_pattern_match` against `libkicommon.so`.
//!
//! `tests/oracle/pattern_oracle.cpp` (built by `tests/oracle/build_c3_oracle.sh`) runs every
//! `EDA_PATTERN_MATCH` class and `EDA_COMBINED_MATCHER` in all five contexts on the patterns of
//! `tests/oracle/c3_patterns.txt` plus every net class pattern found in the `.kicad_pro` files
//! of `qa/data` and `demos`, against the subjects of `tests/oracle/c3_subjects.txt`. The golden
//! file is `tests/golden/c3_pattern.txt`; [`dump_all`] mirrors `main()` of the C++ program.
//! The same driver also covers `JSON_SETTINGS` + `PARAM<T>` loading/storing of a table of
//! documents (sections `settings`, `nested`) and `ExpandTextVars` / `ResolveTextVars` /
//! `FinalizeTextVarExpansion` / `NormalizeFilePathForTextVars` on 4 000 pseudo-random texts
//! (section `textvars`).

use std::collections::BTreeSet;

use std::collections::BTreeMap;

use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_pattern_match::*;
use kicad_core::json::{self, ObjectOrder, Value, dump};
use kicad_core::json_settings::{JsonSettings, JsonSettingsInternals, NestedSettings};
use kicad_core::settings_field;
use kicad_core::settings_parameters::*;
use kicad_core::text_vars::*;

mod c3_common;
use c3_common::{Out, check_golden, esc, json_files};

fn res(r: FindResult) -> String {
    if r.valid() { format!("{},{}", r.start, r.length) } else { "-".into() }
}

fn matcher_line<M: EdaPatternMatch + Default>(name: &str, pattern: &str, subjects: &[String]) -> String {
    let mut m = M::default();
    let ok = m.set_pattern(pattern);
    let mut line = format!(
        "{name} {} {} [{}]",
        esc(pattern.as_bytes()),
        if ok { "ok" } else { "fail" },
        esc(m.pattern().as_bytes())
    );

    for s in subjects {
        line += " ";
        line += &res(m.find(s));
    }

    line
}

fn oracle_file(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/oracle/{name}", env!("CARGO_MANIFEST_DIR"))).expect("oracle data file")
}

/// Patterns of `c3_patterns.txt` followed by the net class patterns of the project files.
pub fn patterns() -> Vec<String> {
    let mut patterns: Vec<String> = oracle_file("c3_patterns.txt").lines().map(String::from).collect();
    let mut seen: BTreeSet<String> = patterns.iter().cloned().collect();
    let root = kicad_qa::kicad_source_root();

    for rel in json_files() {
        if !rel.ends_with(".kicad_pro") {
            continue;
        }

        let bytes = std::fs::read(root.join(&rel)).expect("readable file");
        let Ok(doc) = json::parse_bytes(&bytes, ObjectOrder::Sorted, true) else { continue };
        let Some(Value::Array(a)) = doc.get("net_settings").and_then(|n| n.get("netclass_patterns")) else { continue };

        for e in a {
            let Some(Value::String(p)) = e.get("pattern") else { break };

            if seen.insert(p.clone()) {
                patterns.push(p.clone());
            }
        }
    }

    let deviations = deviations();
    patterns.retain(|p| !deviations.contains(p));
    patterns
}

/// Patterns PCRE2 accepts but the port rejects (`PORT_STATUS.md`, `c3_pattern_deviations.txt`).
fn deviations() -> Vec<String> {
    oracle_file("c3_pattern_deviations.txt").lines().map(String::from).collect()
}

pub fn subjects() -> Vec<String> {
    oracle_file("c3_subjects.txt")
        .lines()
        .map(|l| match json::parse(l, ObjectOrder::Sorted) {
            Ok(Value::String(s)) => s,
            _ => panic!("bad subject line {l}"),
        })
        .collect()
}


// ---- settings (mirrors TEST_SETTINGS / TEST_NESTED of pattern_oracle.cpp) ----

#[derive(Clone, Copy, Debug, PartialEq)]
enum TestEnum {
    A,
    B,
    C,
    D,
}

fn enum_to_int(e: TestEnum) -> i32 {
    e as i32
}

fn enum_from_int(v: i32) -> TestEnum {
    match v {
        0 => TestEnum::A,
        1 => TestEnum::B,
        2 => TestEnum::C,
        _ => TestEnum::D,
    }
}

struct TestSettings {
    b: bool,
    i: i32,
    ir: i32,
    d: f64,
    dr: f64,
    s: String,
    u: u32,
    f: f32,
    e: TestEnum,
    sc: i32,
    mils: i32,
    sl: i64,
    li: Vec<i32>,
    ls: Vec<String>,
    ld: Vec<f64>,
    lp: Vec<String>,
    set: BTreeSet<String>,
    mi: BTreeMap<String, i32>,
    md: BTreeMap<String, f64>,
    ms: BTreeMap<String, String>,
    lam: i32,
    lj: Value,
    path: String,
    ns: String,
}

impl TestSettings {
    fn sentinel() -> Self {
        Self {
            b: true,
            i: -77,
            ir: -77,
            d: -7.25,
            dr: -7.25,
            s: "sentinel".into(),
            u: 77,
            f: -7.5,
            e: TestEnum::D,
            sc: -77,
            mils: -77,
            sl: -77,
            li: vec![77],
            ls: vec!["sentinel".into()],
            ld: vec![7.0],
            lp: vec!["sentinel".into()],
            set: BTreeSet::from(["sentinel".to_string()]),
            mi: BTreeMap::from([("sentinel".to_string(), 77)]),
            md: BTreeMap::from([("sentinel".to_string(), 7.0)]),
            ms: BTreeMap::from([("sentinel".to_string(), "77".to_string())]),
            lam: -77,
            lj: Value::String("sentinel".into()),
            path: "sentinel".into(),
            ns: "sentinel".into(),
        }
    }

    fn dump(&self) -> String {
        let hd = |v: f64| format!("{:016x}", v.to_bits());
        let e = |s: &str| esc(s.as_bytes());
        let mut o = format!(
            "b={} i={} ir={} d={} dr={} s={} u={} f={:08x} e={} sc={} mils={} sl={} li=",
            u8::from(self.b),
            self.i,
            self.ir,
            hd(self.d),
            hd(self.dr),
            e(&self.s),
            self.u,
            self.f.to_bits(),
            self.e as i32,
            self.sc,
            self.mils,
            self.sl
        );
        self.li.iter().for_each(|x| o += &format!("{x};"));
        o += " ls=";
        self.ls.iter().for_each(|x| o += &format!("{};", e(x)));
        o += " ld=";
        self.ld.iter().for_each(|x| o += &format!("{};", hd(*x)));
        o += " lp=";
        self.lp.iter().for_each(|x| o += &format!("{};", e(x)));
        o += " set=";
        self.set.iter().for_each(|x| o += &format!("{};", e(x)));
        o += " mi=";
        self.mi.iter().for_each(|(k, v)| o += &format!("{}:{v};", e(k)));
        o += " md=";
        self.md.iter().for_each(|(k, v)| o += &format!("{}:{};", e(k), hd(*v)));
        o += " ms=";
        self.ms.iter().for_each(|(k, v)| o += &format!("{}:{};", e(k), e(v)));
        o += &format!(
            " lam={} lj={} path={} ns={}",
            self.lam,
            e(&dump(&self.lj, None, ObjectOrder::Sorted)),
            e(&self.path),
            e(&self.ns)
        );
        o
    }
}

fn test_settings(reset: bool) -> JsonSettings<TestSettings> {
    let mut js = JsonSettings::new(2, Some("test.json"));
    js.reset_params_if_missing = reset;

    js.add_param(Box::new(ParamT::new("b", settings_field!(TestSettings, b), false)));
    js.add_param(Box::new(ParamT::new("i", settings_field!(TestSettings, i), 5)));
    js.add_param(Box::new(ParamT::with_range("ir", settings_field!(TestSettings, ir), 10, 0, 100)));
    js.add_param(Box::new(ParamT::new("d", settings_field!(TestSettings, d), 1.5)));
    js.add_param(Box::new(ParamT::with_range("dr", settings_field!(TestSettings, dr), 0.5, 0.0, 1.0)));
    js.add_param(Box::new(ParamT::new("s", settings_field!(TestSettings, s), "def".to_string())));
    js.add_param(Box::new(ParamT::new("u", settings_field!(TestSettings, u), 7u32)));
    js.add_param(Box::new(ParamT::new("f", settings_field!(TestSettings, f), 2.5f32)));
    js.add_param(Box::new(ParamEnum::new(
        "e",
        settings_field!(TestSettings, e),
        TestEnum::B,
        TestEnum::A,
        TestEnum::C,
        enum_to_int,
        enum_from_int,
    )));
    js.add_param(Box::new(ParamScaled::with_range(
        "sc",
        settings_field!(TestSettings, sc),
        250000,
        -1000000000,
        1000000000,
        PCB_IU_SCALE.mm_per_iu,
    )));
    js.add_param(Box::new(ParamScaled::new("mils", settings_field!(TestSettings, mils), 100, 0.001)));
    js.add_param(Box::new(ParamScaled::new("sl", settings_field!(TestSettings, sl), 5i64, 1.0 / 1000.0)));
    js.add_param(Box::new(ParamList::new("li", settings_field!(TestSettings, li), vec![1, 2], false)));
    js.add_param(Box::new(ParamList::new("ls", settings_field!(TestSettings, ls), vec!["x".to_string()], false)));
    js.add_param(Box::new(ParamList::new("ld", settings_field!(TestSettings, ld), vec![0.5], true)));
    js.add_param(Box::new(ParamList::path_list("lp", settings_field!(TestSettings, lp), vec!["a\\b".to_string()])));
    js.add_param(Box::new(ParamSet::new("set", settings_field!(TestSettings, set), BTreeSet::from(["q".to_string()]))));
    js.add_param(Box::new(ParamMap::new("mi", settings_field!(TestSettings, mi), BTreeMap::from([("k".to_string(), 1)]))));
    js.add_param(Box::new(ParamMap::new("md", settings_field!(TestSettings, md), BTreeMap::from([("k".to_string(), 1.5)]))));
    js.add_param(Box::new(ParamMap::wx_string_map(
        "ms",
        settings_field!(TestSettings, ms),
        BTreeMap::from([("k".to_string(), "v".to_string())]),
        false,
    )));
    js.add_param(Box::new(ParamLambda::new("lam", |s: &TestSettings| s.lam, |s: &mut TestSettings, v: i32| s.lam = v.wrapping_mul(2), 9)));
    js.add_param(Box::new(ParamLambda::new(
        "lj",
        |s: &TestSettings| JsonValue(s.lj.clone()),
        |s: &mut TestSettings, v: JsonValue| {
            s.lj = if v.0.is_object() || v.0.is_array() {
                v.0
            } else {
                Value::String(format!("set:{}", dump(&v.0, None, ObjectOrder::Sorted)))
            };
        },
        JsonValue(Value::Null),
    )));
    js.add_param(Box::new(ParamPath::new("path", settings_field!(TestSettings, path), "c:\\x")));
    js.add_param(Box::new(ParamT::new("nested.deep.s", settings_field!(TestSettings, ns), "n".to_string())));

    js.register_migration(
        0,
        1,
        Box::new(|int: &mut JsonSettingsInternals| {
            if let Some(v) = int.get::<i32>("old")? {
                int.set("i", &v)?;
            }

            Ok(true)
        }),
    );

    js.register_migration(
        1,
        2,
        Box::new(|int: &mut JsonSettingsInternals| {
            int.set("migrated", "yes")?;
            Ok(!int.contains("fail_migration")?)
        }),
    );

    js
}

const SETTINGS_DOCS: [&str; 22] = [
    "",
    "{}",
    "not json",
    r#"{"meta":{"version":2},"b":true,"i":12,"ir":50,"d":0.25,"dr":0.75,"s":"hello","u":4000000000,"f":0.1,
        "e":2,"sc":1.27,"mils":0.5,"sl":2.5,"li":[3,4,5],"ls":["p","q"],"ld":[1.5,2.5],"lp":["x\\y","z"],
        "set":["b","a","b"],"mi":{"y":2,"x":1},"md":{"m":0.5},"ms":{"k2":"v2"},"lam":4,"lj":{"q":[1]},
        "path":"d:\\p","nested":{"deep":{"s":"deep"}}}"#,
    r#"{"meta":{"version":2},"b":1,"i":"5","ir":true,"d":true,"dr":"x","s":5,"u":-1,"f":true,"e":7,
        "sc":"x","mils":null,"sl":[],"li":[1,"a"],"ls":"notarray","ld":[],"lp":[1],"set":[1],"mi":{"a":"x"},
        "md":[],"ms":{"a":1},"lam":true,"lj":[1],"path":7,"nested":"str"}"#,
    r#"{"meta":{"version":2},"ir":1000,"dr":2.0,"e":-1,"sc":5000,"sl":1e30,"mils":-3000000,"u":1e10,
        "i":3.9,"li":[1.9,-2.9,true],"ld":[true]}"#,
    r#"{"meta":{"version":2},"i":-3.9,"ir":-0.5,"u":3e9,"e":2.7,"sc":0.0000005,"mils":0.0015,"sl":-2.5,
        "d":5,"dr":-0.0,"f":1e40,"lam":2.9,"ir2":4}"#,
    r#"{"meta":{"version":2},"b":null,"s":null,"li":null,"mi":null,"lj":null,"lam":null,"ms":null,
        "set":null,"path":null,"ld":null}"#,
    r#"{"meta":{"version":0},"old":42,"b":false}"#,
    r#"{"meta":{"version":1},"old":42,"i":8}"#,
    r#"{"meta":{"version":0},"old":"x","fail_migration":true}"#,
    r#"{"meta":{"version":3},"i":99}"#,
    r#"{"meta":{"version":"2"},"i":98}"#,
    r#"{"meta":{"version":-1},"i":97}"#,
    r#"{"meta":{"version":1.9},"i":96}"#,
    r#"{"i":95}"#,
    "\u{FEFF}// comment\n{\"meta\":{\"version\":2},/* c */\"i\":1,\"i\":2,\"s\":\"a\\u00e9\\n\"}",
    r#"{"meta":{"version":2},"mi":{"b":1,"a":2.5,"c":"x","d":4},"ms":{"b":"1","a":2,"c":"3"},"li":[1,2,"x"]}"#,
    r#"{"meta":{"version":2},"nested":{"deep":{"s":["x"]}},"set":["z","é","a"],"lp":["a\\b\\c"]}"#,
    r#"[1,2,3]"#,
    r#""string""#,
    r#"{"meta":{"version":2},"sc":-1.0000005,"mils":-0.0005,"sl":9.3e15,"u":-2.5,"f":-1e-46}"#,
];

const NESTED_DOCS: [&str; 9] = [
    "{}",
    r#"{"sub":{"meta":{"version":1},"a":5,"m":{"q":1}}}"#,
    r#"{"sub":{"meta":{"version":0},"a":5}}"#,
    r#"{"sub":{"meta":{"version":0},"a":5,"throw":1}}"#,
    r#"{"sub":{"a":7}}"#,
    r#"{"sub":"notobject"}"#,
    r#"{"sub":null}"#,
    r#"{"sub":{"meta":{"version":2},"a":"x","m":[]}}"#,
    r#"{"sub":{"meta":{"version":1},"a":3,"m":{"z":26}},"other":1}"#,
];

struct TestNested {
    a: i32,
    m: BTreeMap<String, i32>,
}

fn test_resolver(token: &mut String) -> bool {
    let value = match token.as_str() {
        "VAR" => "value",
        "X" => "5",
        "Y" => "2",
        "NESTED" => "${VAR}",
        "R" => "${R}",
        "A B" => "spaced",
        "ERC_WARNING x" => "assert",
        "value" => "VAL",
        "5" => "five",
        "{" => "brace",
        "E" => "\\${X}",
        _ => return false,
    };

    *token = value.to_string();
    true
}

const TEXT_FRAGS: [&str; 24] = [
    "${",
    "}",
    "\\",
    "VAR",
    "X",
    "Y",
    "$",
    "{",
    "<<<ESC_DOLLAR:",
    "<<<ESC_AT:",
    "ERC_WARNING x",
    "abc",
    " ",
    "NESTED",
    "R",
    "A B",
    "@{",
    "\\${",
    "\\@{",
    "DRC_ERROR",
    "é",
    "E",
    "5",
    "}}",
];

const CONTEXTS: [ResolutionContext; 8] = [
    ResolutionContext::ForCanvas,
    ResolutionContext::ForGui,
    ResolutionContext::ForNetname,
    ResolutionContext::ForErcDrc,
    ResolutionContext::Internal,
    ResolutionContext::Resolved,
    ResolutionContext::RawValue,
    ResolutionContext::ForUri,
];

fn settings_sections(o: &mut Out) {
    o.begin("settings");

    for doc in SETTINGS_DOCS {
        for reset in [true, false] {
            let mut js = test_settings(reset);
            let mut t = TestSettings::sentinel();
            let ok = js.load_from_buffer(doc.as_bytes(), &mut t);
            let mut line = format!(
                "{} {} {}{}",
                if reset { "R" } else { "K" },
                if ok { "ok" } else { "fail" },
                if js.is_future_format { "future " } else { "" },
                t.dump()
            );
            let modified = js.store(&t);
            let contents = match js.format_file_contents() {
                Ok(c) => esc(c.as_bytes()),
                Err(e) => format!("throw {}", e.id),
            };
            line += &format!(" | store={} | {contents} | {}", u8::from(modified), esc(js.format_as_string(&t).as_bytes()));
            o.out(line);
        }
    }

    o.begin("nested");

    for doc in NESTED_DOCS {
        for reset in [true, false] {
            let mut parent = JsonSettingsInternals::new(json::parse_bytes(doc.as_bytes(), ObjectOrder::Sorted, false).unwrap());
            let mut n: NestedSettings<TestNested> = NestedSettings::new(1, "sub");
            n.settings.reset_params_if_missing = reset;
            n.settings.add_param(Box::new(ParamT::new("a", settings_field!(TestNested, a), 3)));
            n.settings.add_param(Box::new(ParamMap::new("m", settings_field!(TestNested, m), BTreeMap::from([("z".to_string(), 26)]))));
            n.settings.register_migration(
                0,
                1,
                Box::new(|int: &mut JsonSettingsInternals| {
                    int.set("a", &100)?;

                    if int.contains("throw")? {
                        return Err(json::JsonError::type_must_be("migration", &Value::Null));
                    }

                    Ok(true)
                }),
            );

            let mut t = TestNested { a: -77, m: BTreeMap::from([("sentinel".to_string(), 77)]) };
            let loaded = n.load_from_parent(Some(&parent), &mut t);
            let mut line = format!("{} {} a={} m=", if reset { "R" } else { "K" }, if loaded { "ok" } else { "fail" }, t.a);
            t.m.iter().for_each(|(k, v)| line += &format!("{k}:{v};"));
            let modified = n.save_to_parent(&mut parent, &t, false);
            line += &format!(" | save={} | {}", u8::from(modified), esc(dump(&parent.doc, None, ObjectOrder::Sorted).as_bytes()));
            t.a = 41;
            let modified = n.save_to_parent(&mut parent, &t, false);
            line += &format!(" | save2={} | {}", u8::from(modified), esc(dump(&parent.doc, None, ObjectOrder::Sorted).as_bytes()));
            o.out(line);
        }
    }

    o.begin("textvars");

    let resolver = test_resolver;
    let mut state: u64 = 0xC3E0;
    let mut next = || -> u32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };

    for _ in 0..4000 {
        let mut src = String::new();
        let n = next() % 9;

        for _ in 0..n {
            src += TEXT_FRAGS[(next() % TEXT_FRAGS.len() as u32) as usize];
        }

        if let Some(dollar) = src.find("${") {
            if src[dollar..].contains('@') {
                continue;
            }
        }

        let mut line = esc(src.as_bytes());

        for ctx in CONTEXTS {
            let mut r = expand_text_vars(&src, Some(&resolver), ctx);
            line += &format!(" | {}", esc(r.as_bytes()));
            finalize_text_var_expansion(&mut r, ctx);
            line += &format!(" ~ {}", esc(r.as_bytes()));
        }

        if !src.contains('@') {
            let mut depth = 0;
            let r = resolve_text_vars(&src, Some(&resolver), &mut depth, &TextVarOptions::default());
            line += &format!(" || {} {depth}", esc(r.as_bytes()));
        }

        line += &format!(" || {}", esc(normalize_file_path_for_text_vars(&src).as_bytes()));
        o.out(line);
    }
}

fn dump_all() -> Out {
    let mut o = Out::default();
    let patterns = patterns();
    let subjects = subjects();

    o.begin("patterns");

    for p in &patterns {
        o.out(esc(p.as_bytes()));
    }

    o.begin("matchers");

    for p in &patterns {
        o.out(matcher_line::<EdaPatternMatchSubstr>("SUBSTR", p, &subjects));
        o.out(matcher_line::<EdaPatternMatchRegex>("REGEX", p, &subjects));
        o.out(matcher_line::<EdaPatternMatchRegex>("REGEX/", &format!("/{p}/"), &subjects));
        o.out(matcher_line::<EdaPatternMatchRegexAnchored>("REGEX_ANCHORED", p, &subjects));
        o.out(matcher_line::<EdaPatternMatchWildcard>("WILDCARD", p, &subjects));
        o.out(matcher_line::<EdaPatternMatchWildcardAnchored>("WILDCARD_ANCHORED", p, &subjects));
        o.out(matcher_line::<EdaPatternMatchRelational>("RELATIONAL", p, &subjects));
    }

    o.begin("combined");

    let contexts = [
        CombinedMatcherContext::LibItem,
        CombinedMatcherContext::Net,
        CombinedMatcherContext::Netclass,
        CombinedMatcherContext::Signal,
        CombinedMatcherContext::Search,
    ];

    for p in &patterns {
        for (ctx, &context) in contexts.iter().enumerate() {
            let m = EdaCombinedMatcher::new(p, context);
            let mut line = format!("{ctx} {}", esc(p.as_bytes()));

            for s in &subjects {
                let (found, triggered, pos) = m.find_detailed(s);
                line += &format!(
                    " {triggered}:{pos}:{}{}{}",
                    u8::from(found),
                    u8::from(m.find(s)),
                    u8::from(m.starts_with(s))
                );
            }

            let mut terms: Vec<SearchTerm> =
                subjects.iter().enumerate().map(|(i, s)| SearchTerm::new(s, (i % 7) as i32 + 1, i % 3 == 0)).collect();
            let mut exact = false;
            let score = m.score_terms(&mut terms, Some(&mut exact));
            line += &format!(" | {score} {}", u8::from(exact));

            let upper: String = p.chars().flat_map(char::to_uppercase).collect();
            let mut own = vec![SearchTerm::new(p, 3, true), SearchTerm::new(&format!("{upper}  "), 2, false)];
            exact = false;
            let score = m.score_terms(&mut own, Some(&mut exact));
            line += &format!(" {score} {} [{}]", u8::from(exact), esc(own[1].text.as_bytes()));
            o.out(line);
        }
    }

    settings_sections(&mut o);

    // Deviations: the port must fail to compile them (C++ compiles them); the section records
    // the C++ lines, which are taken from the golden file after that check.
    o.begin("deviations");

    let golden = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/c3_pattern.txt")).unwrap_or_default();
    let cpp: Vec<&str> = golden.lines().filter_map(|l| l.strip_prefix("E deviations ")).collect();

    for (k, p) in deviations().iter().enumerate() {
        for (j, line) in [
            matcher_line::<EdaPatternMatchRegex>("REGEX/", &format!("/{p}/"), &subjects),
            matcher_line::<EdaPatternMatchRegexAnchored>("REGEX_ANCHORED", p, &subjects),
        ]
        .iter()
        .enumerate()
        {
            let c = cpp.get(2 * k + j).copied().unwrap_or("");

            if c.contains(" ok [") {
                assert!(line.contains(" fail ["), "deviation pattern {p:?} unexpectedly compiles: {line}");
            } else {
                assert_eq!(line, c, "deviation pattern {p:?}");
            }

            o.out(c.to_string());
        }
    }

    o
}

#[test]
fn c3_pattern_oracle_golden() {
    let golden = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/c3_pattern.txt");
    check_golden(golden, &dump_all(), "c3_pattern_rust_full.txt");
}
