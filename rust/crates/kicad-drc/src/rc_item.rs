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

//! Port of the report part of `include/rc_item.h` / `common/rc_item.cpp` (work package
//! DRC-11): `RC_ITEM_RESOLVER`, `RC_ITEM::ShowReport`, `RC_ITEM::GetJsonViolation` and the
//! `RC_ITEM::getSeverityString` / `showCoord` helpers; the `RC_JSON` structs of
//! `include/rc_json_schema.h` with their `to_json`; and `HYPERLINK_DV_RENDERER::StripMarkup` /
//! `ParseRuns` of `common/widgets/wx_data_view_hyperlink_renderer.cpp`.
//!
//! The item data (`RC_ITEM` members) is `kicad_pcb::pcb_marker::RcItem`. `RC_ITEM::m_parent`
//! (the owning marker) is passed explicitly as [`RcItemParent`]. The `RC_TREE_MODEL` (GUI) is
//! not ported.

use std::cell::RefCell;
use std::collections::BTreeSet;

use kicad_core::eda_units::{EdaDataType, ui};
use kicad_core::json::{Map, Value, float_value};
use kicad_core::report_severity::get_severity_string;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Kiid, NILUUID, Severity};
use kicad_geom::Vector2I;
use kicad_pcb::item_ref::ItemRef;
use kicad_pcb::pcb_marker::RcItem;

use crate::item;

// ---- HYPERLINK_DV_RENDERER::StripMarkup ------------------------------------------------------

/// Port of `tryParseMarkdown` (`wx_data_view_hyperlink_renderer.cpp`): parse `[label](url)`
/// starting at `start`; returns `(end, label, href)`. Indices are characters (`wxString`
/// positions are code points on Linux).
fn try_parse_markdown(text: &[char], start: usize) -> Option<(usize, String, String)> {
    if start >= text.len() || text[start] != '[' {
        return None;
    }

    let label_end = text[start + 1..]
        .iter()
        .position(|&c| c == ']')
        .map(|p| p + start + 1)?;

    if label_end + 1 >= text.len() || text[label_end + 1] != '(' {
        return None;
    }

    // Match balanced parens so URLs like "file:///foo (1).pdf" survive.
    let href_start = label_end + 2;
    let mut href_end = None;
    let mut depth = 1;

    for (i, &c) in text.iter().enumerate().skip(href_start) {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;

            if depth == 0 {
                href_end = Some(i);
                break;
            }
        }
    }

    let href_end = href_end?;

    // aText.SubString( aStart + 1, labelEnd - 1 ), aText.SubString( hrefStart, hrefEnd - 1 )
    let label: String = text[start + 1..label_end].iter().collect();
    let href: String = text[href_start..href_end].iter().collect();
    Some((href_end + 1, label, href))
}

/// One run of `HYPERLINK_DV_RENDERER::ParseRuns`: plain text, or a link label with its href.
/// The C++ run also carries the href only when `IsSafeUrl( href )`; that GUI check is not
/// ported (the report writers use the texts only), so `href` is always the parsed target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HyperlinkRun {
    /// `RUN::text`.
    pub text: String,
    /// The parsed link target (empty for plain text).
    pub href: String,
}

/// Port of `HYPERLINK_DV_RENDERER::ParseRuns` (see [`HyperlinkRun`] for the href).
pub fn parse_runs(value: &str) -> Vec<HyperlinkRun> {
    let chars: Vec<char> = value.chars().collect();
    let mut runs = Vec::new();
    let mut pos = 0;
    let mut plain = String::new();

    while pos < chars.len() {
        let parsed = if chars[pos] == '[' {
            try_parse_markdown(&chars, pos)
        } else {
            None
        };

        if let Some((end, label, href)) = parsed {
            if !plain.is_empty() {
                runs.push(HyperlinkRun {
                    text: std::mem::take(&mut plain),
                    href: String::new(),
                });
            }

            runs.push(HyperlinkRun { text: label, href });
            pos = end;
        } else {
            plain.push(chars[pos]);
            pos += 1;
        }
    }

    if !plain.is_empty() {
        runs.push(HyperlinkRun {
            text: plain,
            href: String::new(),
        });
    }

    runs
}

/// Port of `HYPERLINK_DV_RENDERER::StripMarkup`: the text with every `[label](url)` replaced
/// by its label.
pub fn strip_markup(value: &str) -> String {
    parse_runs(value).into_iter().map(|run| run.text).collect()
}

// ---- RC_ITEM_RESOLVER ------------------------------------------------------------------------

/// What the report code reads from a resolved `EDA_ITEM*`.
pub trait RcReportItem {
    /// `m_Uuid`.
    fn uuid(&self) -> Kiid;
    /// `GetPosition()`.
    fn position(&self) -> Vector2I;
    /// `GetItemDescription( aUnitsProvider, aFull )`.
    fn item_description(&self, units: &UnitsProvider<'_>, full: bool) -> String;
}

impl RcReportItem for ItemRef<'_> {
    fn uuid(&self) -> Kiid {
        ItemRef::uuid(self)
    }

    fn position(&self) -> Vector2I {
        ItemRef::position(self)
    }

    fn item_description(&self, units: &UnitsProvider<'_>, full: bool) -> String {
        ItemRef::item_description(self, units, full)
    }
}

/// The pure virtual `RC_ITEM_RESOLVER::doResolveItemById`.
pub trait ResolveItemById {
    /// The item with this (non-nil) id, `None` if there is none.
    fn do_resolve_item_by_id(&self, kiid: &Kiid) -> Option<Box<dyn RcReportItem + '_>>;
}

/// Port of `RC_ITEM_RESOLVER`: a resolver with the cache of ids that did not resolve.
pub struct RcItemResolver<R> {
    inner: R,
    resolution_misses: RefCell<BTreeSet<Kiid>>,
}

impl<R: ResolveItemById> RcItemResolver<R> {
    /// Wraps a `doResolveItemById` implementation.
    pub fn new(inner: R) -> Self {
        RcItemResolver {
            inner,
            resolution_misses: RefCell::new(BTreeSet::new()),
        }
    }

    /// Port of `RC_ITEM_RESOLVER::ResolveItemById`.
    pub fn resolve_item_by_id(&self, kiid: &Kiid) -> Option<Box<dyn RcReportItem + '_>> {
        // This is expected, e.g., when an RC item doesn't have an aux ID.
        if *kiid == NILUUID {
            return None;
        }

        if self.resolution_misses.borrow().contains(kiid) {
            return None;
        }

        let item = self.inner.do_resolve_item_by_id(kiid);

        if item.is_none() {
            self.resolution_misses.borrow_mut().insert(*kiid);
        }

        item
    }
}

// ---- RC_ITEM report text ---------------------------------------------------------------------

/// What `ShowReport` / `GetJsonViolation` read from `RC_ITEM::m_parent` (the marker):
/// `IsTreatedAsExcluded()` and `GetComment()`. `None` = no parent marker.
#[derive(Clone, Copy, Debug)]
pub struct RcItemParent<'a> {
    /// `MARKER_BASE::IsTreatedAsExcluded()`.
    pub treated_as_excluded: bool,
    /// `MARKER_BASE::GetComment()`.
    pub comment: &'a str,
}

/// Port of `RC_ITEM::getSeverityString`.
pub fn severity_string(severity: Severity) -> &'static str {
    get_severity_string(severity)
}

/// Port of `showCoord` (`rc_item.cpp`): `@(x, y)` with `MessageTextFromValue`.
fn show_coord(units: &UnitsProvider<'_>, pos: Vector2I) -> String {
    format!(
        "@({}, {})",
        units.message_text_from_value(f64::from(pos.x), true, EdaDataType::Distance),
        units.message_text_from_value(f64::from(pos.y), true, EdaDataType::Distance)
    )
}

/// Port of `RC_ITEM::getItemDescription( aItem, aIndex, aUnitsProvider )` (not overridden by
/// `DRC_ITEM`).
fn item_description(item: &dyn RcReportItem, units: &UnitsProvider<'_>) -> String {
    item.item_description(units, true)
}

/// Port of `RC_ITEM::ShowReport( aUnitsProvider, aSeverity, aResolver )` for a `DRC_ITEM`
/// (`GetViolatingRuleDesc` of `DRC_ITEM`).
///
/// Note: some customers machine-process these.  So:
/// 1) don't translate
/// 2) try not to re-order or change syntax
/// 3) report settings key (which should be more stable) in addition to message
pub fn show_report<R: ResolveItemById>(
    item: &RcItem,
    parent: Option<RcItemParent<'_>>,
    units: &UnitsProvider<'_>,
    severity: Severity,
    resolver: &RcItemResolver<R>,
) -> String {
    let mut severity = severity_string(severity).to_string();
    let excluded = parent.is_some_and(|p| p.treated_as_excluded);

    if excluded {
        severity += " (excluded)";
    }

    let main_item = resolver.resolve_item_by_id(&item.main_item_id());
    let aux_item = resolver.resolve_item_by_id(&item.aux_item_id());

    let error_message = strip_markup(&item.error_message());
    let rule_desc = strip_markup(&item::violating_rule_desc(item));

    let mut msg = match (&main_item, &aux_item) {
        (Some(main), Some(aux)) => format!(
            "[{}]: {}\n    {}; {}\n    {}: {}\n    {}: {}\n",
            item.settings_key(),
            error_message,
            rule_desc,
            severity,
            show_coord(units, main.position()),
            item_description(main.as_ref(), units),
            show_coord(units, aux.position()),
            item_description(aux.as_ref(), units)
        ),
        (Some(main), None) => format!(
            "[{}]: {}\n    {}; {}\n    {}: {}\n",
            item.settings_key(),
            error_message,
            rule_desc,
            severity,
            show_coord(units, main.position()),
            item_description(main.as_ref(), units)
        ),
        _ => format!(
            "[{}]: {}\n    {}; {}\n",
            item.settings_key(),
            error_message,
            rule_desc,
            severity
        ),
    };

    if let Some(parent) = parent {
        if excluded && !parent.comment.is_empty() {
            msg += &format!("    {}\n", parent.comment);
        }
    }

    msg
}

// ---- RC_JSON (rc_json_schema.h) ---------------------------------------------------------------

/// `RC_JSON::COORDINATE`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JsonCoordinate {
    pub x: f64,
    pub y: f64,
}

/// `RC_JSON::AFFECTED_ITEM`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsonAffectedItem {
    pub uuid: String,
    pub description: String,
    pub pos: JsonCoordinate,
}

/// `RC_JSON::VIOLATION`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsonViolation {
    pub type_: String,
    pub description: String,
    pub severity: String,
    pub items: Vec<JsonAffectedItem>,
    pub excluded: bool,
    /// exclusion comment; if any
    pub comment: String,
}

/// `RC_JSON::IGNORED_CHECK`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsonIgnoredCheck {
    pub key: String,
    pub description: String,
}

/// `RC_JSON::DRC_REPORT` (with the `REPORT_BASE` members; `type` is "drc" but is not
/// serialised).
#[derive(Clone, Debug, PartialEq)]
pub struct JsonDrcReport {
    pub schema: String,
    pub source: String,
    pub date: String,
    pub kicad_version: String,
    pub type_: String,
    pub coordinate_units: String,
    pub violations: Vec<JsonViolation>,
    pub unconnected_items: Vec<JsonViolation>,
    pub schematic_parity: Vec<JsonViolation>,
    pub included_severities: Vec<String>,
    pub ignored_checks: Vec<JsonIgnoredCheck>,
}

impl Default for JsonDrcReport {
    /// `DRC_REPORT() { type = wxS( "drc" ); }`.
    fn default() -> Self {
        JsonDrcReport {
            schema: String::new(),
            source: String::new(),
            date: String::new(),
            kicad_version: String::new(),
            type_: "drc".to_string(),
            coordinate_units: String::new(),
            violations: Vec::new(),
            unconnected_items: Vec::new(),
            schematic_parity: Vec::new(),
            included_severities: Vec::new(),
            ignored_checks: Vec::new(),
        }
    }
}

fn string_value(s: &str) -> Value {
    Value::String(s.to_string())
}

impl JsonCoordinate {
    /// `NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE( COORDINATE, x, y )`.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("x".to_string(), float_value(self.x));
        map.insert("y".to_string(), float_value(self.y));
        Value::Object(map)
    }
}

impl JsonAffectedItem {
    /// `NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE( AFFECTED_ITEM, uuid, description, pos )`.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("uuid".to_string(), string_value(&self.uuid));
        map.insert("description".to_string(), string_value(&self.description));
        map.insert("pos".to_string(), self.pos.to_json());
        Value::Object(map)
    }
}

impl JsonViolation {
    /// Port of `RC_JSON::to_json( json&, const VIOLATION& )`: `excluded` and `comment` only for
    /// excluded violations.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("type".to_string(), string_value(&self.type_));
        map.insert("description".to_string(), string_value(&self.description));
        map.insert("severity".to_string(), string_value(&self.severity));
        map.insert(
            "items".to_string(),
            Value::Array(self.items.iter().map(JsonAffectedItem::to_json).collect()),
        );

        if self.excluded {
            map.insert("excluded".to_string(), Value::Bool(self.excluded));
            map.insert("comment".to_string(), string_value(&self.comment));
        }

        Value::Object(map)
    }
}

impl JsonIgnoredCheck {
    /// `NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE( IGNORED_CHECK, key, description )`.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("key".to_string(), string_value(&self.key));
        map.insert("description".to_string(), string_value(&self.description));
        Value::Object(map)
    }
}

impl JsonDrcReport {
    /// `NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE( DRC_REPORT, $schema, source, date, kicad_version,
    /// violations, unconnected_items, schematic_parity, coordinate_units, included_severities,
    /// ignored_checks )`. Dump it with sorted keys (`nlohmann::json`).
    pub fn to_json(&self) -> Value {
        let violations =
            |list: &[JsonViolation]| Value::Array(list.iter().map(JsonViolation::to_json).collect());

        let mut map = Map::new();
        map.insert("$schema".to_string(), string_value(&self.schema));
        map.insert("source".to_string(), string_value(&self.source));
        map.insert("date".to_string(), string_value(&self.date));
        map.insert("kicad_version".to_string(), string_value(&self.kicad_version));
        map.insert("violations".to_string(), violations(&self.violations));
        map.insert(
            "unconnected_items".to_string(),
            violations(&self.unconnected_items),
        );
        map.insert(
            "schematic_parity".to_string(),
            violations(&self.schematic_parity),
        );
        map.insert(
            "coordinate_units".to_string(),
            string_value(&self.coordinate_units),
        );
        map.insert(
            "included_severities".to_string(),
            Value::Array(
                self.included_severities
                    .iter()
                    .map(|s| string_value(s))
                    .collect(),
            ),
        );
        map.insert(
            "ignored_checks".to_string(),
            Value::Array(
                self.ignored_checks
                    .iter()
                    .map(JsonIgnoredCheck::to_json)
                    .collect(),
            ),
        );
        Value::Object(map)
    }
}

/// `EDA_UNIT_UTILS::UI::ToUserUnit( iuScale, userUnits, value )` of a report position.
fn to_user_unit(units: &UnitsProvider<'_>, value: i32) -> f64 {
    ui::to_user_unit(units.iu_scale(), units.user_units(), f64::from(value))
}

fn affected_item(item: &dyn RcReportItem, units: &UnitsProvider<'_>) -> JsonAffectedItem {
    let pos = item.position();

    JsonAffectedItem {
        description: item_description(item, units),
        uuid: item.uuid().as_string(),
        pos: JsonCoordinate {
            x: to_user_unit(units, pos.x),
            y: to_user_unit(units, pos.y),
        },
    }
}

/// Port of `RC_ITEM::GetJsonViolation( aViolation, aUnitsProvider, aSeverity, aResolver )`.
pub fn json_violation<R: ResolveItemById>(
    item: &RcItem,
    parent: Option<RcItemParent<'_>>,
    units: &UnitsProvider<'_>,
    severity: Severity,
    resolver: &RcItemResolver<R>,
) -> JsonViolation {
    let mut violation = JsonViolation {
        severity: severity_string(severity).to_string(),
        description: strip_markup(&item.error_message()),
        type_: item.settings_key().to_string(),
        ..JsonViolation::default()
    };

    match parent {
        Some(parent) if parent.treated_as_excluded => {
            violation.excluded = true;
            violation.comment = parent.comment.to_string();
        }
        _ => violation.excluded = false,
    }

    let main_item = resolver.resolve_item_by_id(&item.main_item_id());
    let aux_item = resolver.resolve_item_by_id(&item.aux_item_id());

    if let Some(main) = &main_item {
        violation.items.push(affected_item(main.as_ref(), units));
    }

    if let Some(aux) = &aux_item {
        violation.items.push(affected_item(aux.as_ref(), units));
    }

    violation
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_core::eda_units::EdaUnits;
    use kicad_core::json::{ObjectOrder, dump};
    use kicad_core::{KiidGenerator, PCB_IU_SCALE};
    use kicad_pcb::drc_api::{DRCE_CLEARANCE, DRCE_UNCONNECTED_ITEMS, DrcRuleInfo};

    #[test]
    fn strip_markup_cases() {
        assert_eq!(strip_markup("plain"), "plain");
        assert_eq!(strip_markup("see [docs](https://x.org/a) now"), "see docs now");
        assert_eq!(strip_markup("[a](file:///foo (1).pdf)!"), "a!");
        assert_eq!(strip_markup("[a]b"), "[a]b");
        assert_eq!(strip_markup("[a](unterminated"), "[a](unterminated");
        assert_eq!(strip_markup("[](x)"), "");
        assert_eq!(strip_markup("[a[b](c)"), "a[b");
        assert_eq!(strip_markup("x [y"), "x [y");
        assert_eq!(strip_markup("[x]"), "[x]");
        assert_eq!(strip_markup("ü[ä](ö)ß"), "üäß");
        assert_eq!(
            parse_runs("a[b](c)"),
            vec![
                HyperlinkRun {
                    text: "a".into(),
                    href: String::new()
                },
                HyperlinkRun {
                    text: "b".into(),
                    href: "c".into()
                }
            ]
        );
    }

    /// A stand-in item for the resolver.
    struct Fake {
        uuid: Kiid,
        pos: Vector2I,
        desc: &'static str,
    }

    impl RcReportItem for &Fake {
        fn uuid(&self) -> Kiid {
            self.uuid
        }
        fn position(&self) -> Vector2I {
            self.pos
        }
        fn item_description(&self, _units: &UnitsProvider<'_>, full: bool) -> String {
            assert!(full);
            self.desc.to_string()
        }
    }

    struct FakeResolver(Vec<Fake>);

    impl ResolveItemById for FakeResolver {
        fn do_resolve_item_by_id(&self, kiid: &Kiid) -> Option<Box<dyn RcReportItem + '_>> {
            self.0
                .iter()
                .find(|f| f.uuid == *kiid)
                .map(|f| Box::new(f) as Box<dyn RcReportItem + '_>)
        }
    }

    fn fixture() -> (RcItemResolver<FakeResolver>, Kiid, Kiid) {
        let mut generator = KiidGenerator::default();
        let a = Kiid::new(&mut generator);
        let b = Kiid::new(&mut generator);
        let resolver = RcItemResolver::new(FakeResolver(vec![
            Fake {
                uuid: a,
                pos: Vector2I::new(148_500_000, 92_710_000),
                desc: "Track [GND] on F.Cu, length 1.0000 mm",
            },
            Fake {
                uuid: b,
                pos: Vector2I::new(-1_000_000, 100_000_000),
                desc: "Pad 1 [GND] of U1 on F.Cu",
            },
        ]));
        (resolver, a, b)
    }

    /// Expected text derived from the format strings of `RC_ITEM::ShowReport`.
    #[test]
    fn show_report_formats() {
        let (resolver, a, b) = fixture();
        let units = UnitsProvider::new(&PCB_IU_SCALE, EdaUnits::Mm);

        let mut item = crate::item::create(DRCE_CLEARANCE).unwrap();
        item.set_error_detail("(netclass 'Default' clearance 0.2000 mm; actual 0.1500 mm)");
        item.violating_rule = Some(DrcRuleInfo {
            name: "netclass 'Default'".into(),
            ..DrcRuleInfo::default()
        });
        item.set_item_ids(vec![a, b]);

        let text = show_report(&item, None, &units, Severity::Error, &resolver);
        assert_eq!(
            text,
            "[clearance]: Clearance violation (netclass 'Default' clearance 0.2000 mm; actual \
             0.1500 mm)\n    Rule: netclass 'Default'; error\n    @(148.5000 mm, 92.7100 mm): \
             Track [GND] on F.Cu, length 1.0000 mm\n    @(-1.0000 mm, 100.0000 mm): Pad 1 [GND] \
             of U1 on F.Cu\n"
        );

        // Excluded with a comment; aux item that does not resolve.
        let unknown = Kiid::parse("00000000-0000-0000-0000-0000000000ff").unwrap();
        item.set_item_ids(vec![a, unknown]);
        item.violating_rule = None;
        let parent = RcItemParent {
            treated_as_excluded: true,
            comment: "known issue",
        };
        let text = show_report(&item, Some(parent), &units, Severity::Warning, &resolver);
        assert!(text.ends_with(
            "    Local override; warning (excluded)\n    @(148.5000 mm, 92.7100 mm): Track \
             [GND] on F.Cu, length 1.0000 mm\n    known issue\n"
        ));

        // No resolvable main item: two lines only; mils.
        let units = UnitsProvider::new(&PCB_IU_SCALE, EdaUnits::Mils);
        let mut item = crate::item::create(DRCE_UNCONNECTED_ITEMS).unwrap();
        item.set_item_ids(vec![unknown, b]);
        let text = show_report(&item, None, &units, Severity::Error, &resolver);
        assert_eq!(
            text,
            "[unconnected_items]: Missing connection between items\n    Local override; error\n"
        );
        item.set_item_ids(vec![b]);
        let text = show_report(&item, None, &units, Severity::Error, &resolver);
        assert!(text.ends_with("    @(-39.37 mils, 3937.01 mils): Pad 1 [GND] of U1 on F.Cu\n"));
    }

    /// `GetJsonViolation` plus the nlohmann layout (sorted keys, 4-space indent, `.0` doubles).
    #[test]
    fn json_violation_layout() {
        let (resolver, a, _b) = fixture();
        let units = UnitsProvider::new(&PCB_IU_SCALE, EdaUnits::Mm);
        let mut item = crate::item::create(DRCE_CLEARANCE).unwrap();
        item.set_error_message("Clearance [violation](https://k.org)");
        item.set_item_ids(vec![a]);

        let v = json_violation(&item, None, &units, Severity::Error, &resolver);
        assert!(!v.excluded);
        let text = dump(&v.to_json(), Some(4), ObjectOrder::Sorted);
        assert_eq!(
            text,
            "{\n    \"description\": \"Clearance violation\",\n    \"items\": [\n        {\n            \
             \"description\": \"Track [GND] on F.Cu, length 1.0000 mm\",\n            \"pos\": {\n                \
             \"x\": 148.5,\n                \"y\": 92.71\n            },\n            \"uuid\": \"" .to_string()
                + &a.as_string()
                + "\"\n        }\n    ],\n    \"severity\": \"error\",\n    \"type\": \"clearance\"\n}"
        );

        let parent = RcItemParent {
            treated_as_excluded: true,
            comment: "",
        };
        let v = json_violation(&item, Some(parent), &units, Severity::Warning, &resolver);
        let text = dump(&v.to_json(), None, ObjectOrder::Sorted);
        assert!(text.starts_with("{\"comment\":\"\",\"description\""));
        assert!(text.contains("\"excluded\":true"));

        let mut report = JsonDrcReport::default();
        report.coordinate_units = "mm".into();
        assert_eq!(report.type_, "drc");
        let text = dump(&report.to_json(), None, ObjectOrder::Sorted);
        assert_eq!(
            text,
            "{\"$schema\":\"\",\"coordinate_units\":\"mm\",\"date\":\"\",\"ignored_checks\":[],\
             \"included_severities\":[],\"kicad_version\":\"\",\"schematic_parity\":[],\
             \"source\":\"\",\"unconnected_items\":[],\"violations\":[]}"
        );
    }

    #[test]
    fn resolver_caches_misses() {
        let (resolver, a, _) = fixture();
        let unknown = Kiid::parse("00000000-0000-0000-0000-0000000000ff").unwrap();
        assert!(resolver.resolve_item_by_id(&NILUUID).is_none());
        assert!(resolver.resolve_item_by_id(&unknown).is_none());
        assert!(resolver.resolution_misses.borrow().contains(&unknown));
        assert!(resolver.resolve_item_by_id(&a).is_some());
    }
}
