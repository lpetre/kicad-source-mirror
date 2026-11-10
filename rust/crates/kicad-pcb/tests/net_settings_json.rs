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

//! Package SET-3: the net settings JSON parameters and migrations
//! (`kicad_pcb::project::net_settings_json`).
//!
//! * `oracle_documents` replays `tests/golden/net_settings_json.txt`, written by
//!   `tests/oracle/net_settings_json_golden.sh` from the real `NET_SETTINGS` of the reference
//!   build: the hand-written documents of `tests/data/net_settings_json` (every migration, error
//!   and conversion path; full output including the written file) and every `.kicad_pro` under
//!   `qa/` and `demos/` (loaded classes in full, hashes of the lists and of the written file).
//! * The JSON cases of `qa/tests/common/test_net_settings.cpp`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use kicad_core::json::{self, ObjectOrder, Value};
use kicad_core::json_settings::{JsonSettings, JsonSettingsInternals};
use kicad_pcb::net_settings::{NetChainSource, NetSettings};
use kicad_pcb::netclass::Netclass;
use kicad_pcb::project::net_settings_json::NetSettingsJson;

fn kicad_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;

    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }

    format!("{h:016x}")
}

fn opt(v: Option<i32>) -> String {
    v.map_or("~".to_string(), |v| v.to_string())
}

fn class_line(nc: &Netclass) -> String {
    format!(
        "class\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        nc.get_name(),
        u8::from(nc.is_default()),
        nc.priority,
        opt(nc.clearance),
        opt(nc.track_width),
        opt(nc.via_dia),
        opt(nc.via_drill),
        opt(nc.uvia_dia),
        opt(nc.uvia_drill),
        opt(nc.diff_pair_width),
        opt(nc.diff_pair_gap),
        opt(nc.diff_pair_via_gap),
        opt(nc.wire_width),
        opt(nc.bus_width),
        opt(nc.line_style),
        nc.pcb_color.to_json_string(),
        nc.schematic_color.to_json_string(),
        nc.tuning_profile,
        nc.description
    )
}

fn list(out: &mut Vec<String>, tag: &str, lines: Vec<String>, full: bool) {
    let all: String = lines.iter().map(|l| format!("{l}\n")).collect();
    out.push(format!("{tag}s\t{}\t{}", lines.len(), fnv(&all)));

    if full {
        out.extend(lines.into_iter().map(|l| format!("{tag}\t{l}")));
    }
}

/// The Rust counterpart of `run()` in `tests/oracle/net_settings_json_oracle.cpp`.
fn run_document(rel: &str, contents: &[u8], full: bool) -> Vec<String> {
    let mut out = vec![format!("file\t{rel}")];

    // PARENT: JSON_SETTINGS( "project", SETTINGS_LOC::NONE, 4, ... ) without parameters.
    let mut parent = JsonSettings::<()>::new(4, Some("project.json"));
    let parent_ok = parent.load_from_buffer(contents, &mut ());

    let mut nsj = NetSettingsJson::new("net_settings");
    let mut ns = NetSettings::default();
    let ok = nsj.load_from_parent(Some(&parent.internals), &mut ns);

    let ver = nsj
        .internals()
        .internals_get::<i32>("meta.version")
        .map_or("~".to_string(), |v| v.to_string());

    out.push(format!(
        "load\t{}\t{}\t{ver}",
        u8::from(parent_ok),
        u8::from(ok)
    ));
    out.push(class_line(&ns.default_netclass));

    for (name, nc) in &ns.netclasses {
        out.push(format!("key\t{name}"));
        out.push(class_line(nc));
    }

    let patterns = ns
        .netclass_pattern_assignments
        .iter()
        .map(|(m, nc)| format!("{}\t{nc}", m.pattern()))
        .collect();
    list(&mut out, "pattern", patterns, full);

    let labels = ns
        .netclass_label_assignments
        .iter()
        .map(|(net, classes)| {
            let mut l = net.clone();

            for c in classes {
                l.push('\t');
                l.push_str(c);
            }

            l
        })
        .collect();
    list(&mut out, "label", labels, full);

    let colors = ns
        .net_color_assignments
        .iter()
        .map(|(net, c)| format!("{net}\t{}", c.to_json_string()))
        .collect();
    list(&mut out, "color", colors, full);

    let chain = |m: &BTreeMap<String, String>| m.iter().map(|(k, v)| format!("{k}\t{v}")).collect();
    list(&mut out, "chainclass", chain(&ns.net_chain_classes), full);
    list(
        &mut out,
        "chainnetclass",
        chain(&ns.net_chain_net_classes),
        full,
    );

    // flushToStore(): nested settings first, then the parent's own parameters.
    let mut flush = |nsj: &mut NetSettingsJson, parent: &mut JsonSettings<()>| {
        let mut modified = nsj.save_to_parent(&mut parent.internals, &ns, false);
        modified |= parent.store(&());
        modified
    };

    let modified = flush(&mut nsj, &mut parent);
    let text = parent
        .format_file_contents()
        .unwrap_or_else(|_| "<exception>".to_string());

    out.push(format!(
        "store\t{}\t{}\t{}\t{}",
        u8::from(modified),
        u8::from(text.as_bytes() == contents),
        text.len(),
        fnv(&text)
    ));

    if full {
        out.extend(text.lines().map(|l| format!("o\t{l}")));
    }

    let modified2 = flush(&mut nsj, &mut parent);
    let text2 = parent
        .format_file_contents()
        .unwrap_or_else(|_| "<exception>".to_string());
    out.push(format!(
        "store2\t{}\t{}",
        u8::from(modified2),
        u8::from(text2 == text)
    ));

    out
}

/// Replays the golden written by `tests/oracle/net_settings_json_golden.sh` (C++ `NET_SETTINGS`
/// on the reference build) for every document it lists.
#[test]
fn oracle_documents() {
    let golden = include_str!("golden/net_settings_json.txt");
    let root = kicad_root();
    let mut blocks: Vec<(String, Vec<&str>)> = Vec::new();

    for line in golden.lines() {
        if let Some(rel) = line.strip_prefix("file\t") {
            blocks.push((rel.to_string(), Vec::new()));
        }

        blocks.last_mut().expect("golden starts with a file line").1.push(line);
    }

    let mut mismatches = Vec::new();
    let mut identical_files = 0;

    for (rel, expected) in &blocks {
        let contents = std::fs::read(root.join(rel)).expect("corpus file");
        let full = rel.starts_with("rust/");
        let actual = run_document(rel, &contents, full);

        if actual.len() != expected.len() || actual.iter().zip(expected).any(|(a, e)| a != e) {
            let first = actual
                .iter()
                .zip(expected)
                .position(|(a, e)| a != e)
                .unwrap_or(actual.len().min(expected.len()));
            mismatches.push(format!(
                "{rel}: line {first}\n  rust {:?}\n  c++  {:?}",
                actual.get(first),
                expected.get(first)
            ));
        }

        if expected
            .iter()
            .any(|l| l.starts_with("store\t") && l.split('\t').nth(2) == Some("1"))
        {
            identical_files += 1;
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {} documents differ:\n{}",
        mismatches.len(),
        blocks.len(),
        mismatches.join("\n")
    );
    assert!(blocks.len() > 400, "{} documents", blocks.len());
    assert!(identical_files > 0);
}

// ---- qa/tests/common/test_net_settings.cpp ------------------------------------------------

/// `source.Store(); FormatAsString()`, parsed, `sink.Internals()->CloneFrom(); sink.Load()`.
fn reparse(source_json: &mut NetSettingsJson, source: &NetSettings) -> Value {
    source_json.store(source);
    json::parse(&source_json.format_as_string(source), ObjectOrder::Sorted).unwrap()
}

fn load_into(sink: &mut NetSettings, doc: &Value) -> NetSettingsJson {
    let mut sink_json = NetSettingsJson::new("");
    *sink_json.internals_mut() = JsonSettingsInternals::new(doc.clone());
    sink_json.load(sink);
    sink_json
}

#[test]
fn net_chain_classes_json_round_trip() {
    let mut source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");

    source.set_net_chain_class("CHAIN_A", "Default");
    source.set_net_chain_class("CHAIN_B", "HighSpeed");
    source.set_net_chain_class("CHAIN_WITH_SPACES and: punctuation!", "Power");
    source.set_net_chain_class("Unicode_éèê", "RF_µwave");

    let reparsed = reparse(&mut source_json, &source);

    assert!(reparsed["net_chain_classes"].is_object());

    let mut sink = NetSettings::default();

    // Seed the sink with a stale entry so a no-op reader would still leave it behind;
    // the read lambda must clear() before applying the loaded map.
    sink.set_net_chain_class("STALE_CHAIN", "Default");
    load_into(&mut sink, &reparsed);

    assert_eq!(sink.net_chain_classes(), source.net_chain_classes());
    assert_eq!(sink.get_net_chain_class("CHAIN_A"), "Default");
    assert_eq!(sink.get_net_chain_class("CHAIN_B"), "HighSpeed");
    assert_eq!(
        sink.get_net_chain_class("CHAIN_WITH_SPACES and: punctuation!"),
        "Power"
    );
    assert_eq!(sink.get_net_chain_class("Unicode_éèê"), "RF_µwave");
    assert!(sink.get_net_chain_class("STALE_CHAIN").is_empty());
    assert!(source == sink);
}

#[test]
fn net_chain_classes_json_round_trip_empty() {
    let source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");
    let reparsed = reparse(&mut source_json, &source);

    assert!(reparsed["net_chain_classes"].is_object());
    assert!(reparsed["net_chain_classes"].as_object().unwrap().is_empty());

    let mut sink = NetSettings::default();
    sink.set_net_chain_class("STALE", "Default");
    load_into(&mut sink, &reparsed);

    assert!(sink.net_chain_classes().is_empty());
}

#[test]
fn net_chain_classes_json_drops_empty_values() {
    let mut source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");
    source.set_net_chain_class("KEEP", "Default");

    let mut reparsed = reparse(&mut source_json, &source);
    assert_eq!(reparsed["net_chain_classes"].as_object().unwrap().len(), 1);

    // Inject a hand-crafted empty-value entry to confirm the reader rejects it.
    reparsed["net_chain_classes"]["EMPTY"] = Value::String(String::new());

    let mut sink = NetSettings::default();
    load_into(&mut sink, &reparsed);

    assert_eq!(sink.net_chain_classes().len(), 1);
    assert_eq!(sink.get_net_chain_class("KEEP"), "Default");
    assert!(sink.get_net_chain_class("EMPTY").is_empty());
}

#[test]
fn chain_netclasses_json_round_trip() {
    let mut source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");

    source.set_net_chain_net_class("CHAIN_A", "HighSpeed");
    source.set_net_chain_net_class("Unicode_éèê", "RF_µwave");

    let mut reparsed = reparse(&mut source_json, &source);
    assert!(reparsed["net_chain_netclasses"].is_object());

    // A hand-crafted empty value must be dropped, matching SetNetChainNetClass's clear
    // semantics.
    reparsed["net_chain_netclasses"]["EMPTY"] = Value::String(String::new());

    let mut sink = NetSettings::default();

    // A no-op reader would leave this behind; the read lambda must clear() first.
    sink.set_net_chain_net_class("STALE_CHAIN", "Default");
    load_into(&mut sink, &reparsed);

    assert_eq!(sink.net_chain_net_classes(), source.net_chain_net_classes());
    assert_eq!(sink.get_net_chain_net_class("CHAIN_A"), "HighSpeed");
    assert_eq!(sink.get_net_chain_net_class("Unicode_éèê"), "RF_µwave");
    assert!(sink.get_net_chain_net_class("EMPTY").is_empty());
    assert!(sink.get_net_chain_net_class("STALE_CHAIN").is_empty());
    assert!(source == sink);
}

/// `RemovedChainNetclassIsDroppedFromProjectFile`, with the file kept in memory: the parent
/// is a parameterless `JSON_SETTINGS` (`TEST_PROJECT_SETTINGS`), `SaveToFile` is
/// `flushToStore` + `formatFileContents`, reopening is `LoadFromBuffer` + the nested load.
#[test]
fn removed_chain_netclass_is_dropped_from_project_file() {
    let save = |parent: &mut JsonSettings<()>, nsj: &mut NetSettingsJson, ns: &NetSettings| {
        nsj.save_to_parent(&mut parent.internals, ns, false);
        parent.store(&());
        parent.format_file_contents().unwrap()
    };

    let contents = {
        let mut parent = JsonSettings::<()>::new(0, Some("project.json"));
        let mut nsj = NetSettingsJson::new("net_settings");
        let mut ns = NetSettings::default();

        ns.set_net_chain_class("CHAIN_A", "DDR");
        ns.set_net_chain_class("CHAIN_B", "DDR");
        ns.set_net_chain_net_class("CHAIN_A", "HighSpeed");
        ns.set_net_chain_net_class("CHAIN_B", "Power");

        save(&mut parent, &mut nsj, &ns)
    };

    // Reopening from disk is what gives the parent its baseline; without it the merge has
    // nothing to resurrect and the defect stays invisible.
    let contents = {
        let mut parent = JsonSettings::<()>::new(0, Some("project.json"));
        assert!(parent.load_from_buffer(contents.as_bytes(), &mut ()));

        let mut nsj = NetSettingsJson::new("net_settings");
        let mut ns = NetSettings::default();
        assert!(nsj.load_from_parent(Some(&parent.internals), &mut ns));
        assert_eq!(ns.net_chain_net_classes().len(), 2);
        assert_eq!(ns.net_chain_classes().len(), 2);

        ns.set_net_chain_net_class("CHAIN_B", "");
        ns.set_net_chain_class("CHAIN_B", "");

        save(&mut parent, &mut nsj, &ns)
    };

    let on_disk = json::parse(&contents, ObjectOrder::Sorted).unwrap();
    let net_chain_net_classes = on_disk["net_settings"]["net_chain_netclasses"]
        .as_object()
        .unwrap();
    let net_chain_classes = on_disk["net_settings"]["net_chain_classes"]
        .as_object()
        .unwrap();

    assert!(net_chain_net_classes.contains_key("CHAIN_A"));
    assert!(!net_chain_net_classes.contains_key("CHAIN_B"));
    assert!(net_chain_classes.contains_key("CHAIN_A"));
    assert!(!net_chain_classes.contains_key("CHAIN_B"));
}

fn single_class(name: &str, nc: Netclass) -> BTreeMap<String, Arc<Netclass>> {
    BTreeMap::from([(name.to_string(), Arc::new(nc))])
}

fn edit_default(ns: &mut NetSettings, f: impl FnOnce(&mut Netclass)) {
    // GetDefaultNetclass()->SetClearance(...): an in-place edit through the shared pointer.
    let mut nc = (*ns.default_netclass).clone();
    f(&mut nc);
    ns.default_netclass = Arc::new(nc);
}

#[test]
fn copy_from_produces_equal_instance() {
    let mut source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");

    edit_default(&mut source, |nc| {
        nc.clearance = Some(250000);
        nc.track_width = Some(200000);
        nc.description = "Project default".to_string();
    });

    let mut high_speed = Netclass::new("HighSpeed", false);
    high_speed.clearance = Some(150000);
    high_speed.track_width = Some(127000);
    source.set_netclasses(single_class("HighSpeed", high_speed));

    source.set_netclass_pattern_assignment("DDR_*", "HighSpeed");
    source.set_netclass_label_assignment("EXACT_NET", BTreeSet::from(["HighSpeed".to_string()]));
    source.set_net_chain_class("DDR_BUS", "HighSpeed");

    let mut sink = NetSettings::default();
    let mut sink_json = NetSettingsJson::new("");
    sink_json.copy_from(&mut sink, &mut source_json, &source);

    assert!(source == sink);

    // Verify the deep copy detached pointer ownership: editing source after
    // CopyFrom must not affect sink.
    edit_default(&mut source, |nc| nc.clearance = Some(999999));

    assert!(source != sink);
    assert_eq!(sink.default_netclass.clearance(), 250000);
}

#[test]
fn copy_from_drops_pre_existing_sink_state() {
    let mut source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");
    edit_default(&mut source, |nc| nc.clearance = Some(100000));

    let mut sink = NetSettings::default();
    let mut sink_json = NetSettingsJson::new("");
    edit_default(&mut sink, |nc| nc.clearance = Some(999999));

    sink.set_netclasses(single_class(
        "StaleClass",
        Netclass::new("StaleClass", false),
    ));
    sink.set_netclass_pattern_assignment("STALE_*", "StaleClass");
    sink.set_net_chain_class("STALE_CHAIN", "StaleClass");

    sink_json.copy_from(&mut sink, &mut source_json, &source);

    assert!(source == sink);
    assert_eq!(sink.default_netclass.clearance(), 100000);
    assert!(sink.netclasses().is_empty());
    assert!(sink.netclass_pattern_assignments().is_empty());
    assert!(sink.net_chain_classes().is_empty());
}

#[test]
fn copy_from_clears_stale_derived_caches() {
    let mut sink = NetSettings::default();
    let mut sink_json = NetSettingsJson::new("");

    sink.set_netclasses(single_class(
        "StalePower",
        Netclass::new("StalePower", false),
    ));
    sink.set_netclass_pattern_assignment("VCC_*", "StalePower");

    // Warm the effective-class cache for a net that resolves via the stale pattern.
    sink.get_effective_netclass("VCC_3V3");
    assert!(sink.has_effective_netclass("VCC_3V3"));

    let source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");
    sink_json.copy_from(&mut sink, &mut source_json, &source);

    // The stale-pattern cache entry must NOT survive.
    assert!(!sink.has_effective_netclass("VCC_3V3"));
    assert!(sink.composite_netclasses().is_empty());
}

/// `resolvesToNetclass`: the effective class or one of its constituents has the name.
fn resolves_to_netclass(settings: &mut NetSettings, net_name: &str, expected: &str) -> bool {
    let resolved = settings.get_effective_netclass(net_name);

    if resolved.get_name() == expected {
        return true;
    }

    resolved
        .constituent_netclasses()
        .iter()
        .any(|c| c.get_name() == expected)
}

#[test]
fn copy_from_clears_stale_chain_derived_patterns() {
    let mut sink = NetSettings::default();
    let mut sink_json = NetSettingsJson::new("");

    sink.set_netclasses(single_class(
        "HighSpeed",
        Netclass::new("HighSpeed", false),
    ));
    sink.set_chain_pattern_assignment(NetChainSource::Schematic, "DDR_DQ0", "HighSpeed");

    assert!(resolves_to_netclass(&mut sink, "DDR_DQ0", "HighSpeed"));

    let source = NetSettings::default();
    let mut source_json = NetSettingsJson::new("");
    sink_json.copy_from(&mut sink, &mut source_json, &source);

    assert!(!resolves_to_netclass(&mut sink, "DDR_DQ0", "HighSpeed"));
}

// CopyFromSelfIsNoop: not expressible, `copy_from` cannot borrow the same settings twice.
