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

//! Port of `pcbnew/drc/drc_report.h` / `drc_report.cpp` (`DRC_REPORT::WriteTextReport`,
//! `WriteJsonReport`) and, through [`crate::rc_item`], `include/rc_json_schema.h`. Work package
//! DRC-11.
//!
//! The writers return the file content; the caller writes it (`fopen` / `std::ofstream` in
//! C++, whose failure is the `false` return). Things C++ reads from the environment are
//! parameters: the creation date (`GetISO8601CurrentDateTime()`) and the KiCad version
//! (`GetMajorMinorPatchVersion()`, [`KICAD_MAJOR_MINOR_PATCH_VERSION`] for this tree).

use kicad_core::eda_units::{EdaDataType, EdaUnits, get_label};
use kicad_core::json::{ObjectOrder, dump_stream};
use kicad_core::report_severity::format_severities;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{PCB_IU_SCALE, Severity};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::pcb_marker::PcbMarker;

use crate::item::{BoardRcItemResolver, DrcItemsProvider, items_with_severities};
use crate::rc_item::{
    JsonDrcReport, JsonIgnoredCheck, JsonViolation, RcItemParent, json_violation, show_report,
    strip_markup,
};

/// `KICAD_MAJOR_MINOR_PATCH_VERSION` of this source tree (`GetMajorMinorPatchVersion()`), the
/// `kicad_version` of the JSON report.
pub const KICAD_MAJOR_MINOR_PATCH_VERSION: &str = "10.99.0";

/// `$schema` of the JSON report.
pub const DRC_JSON_SCHEMA_URL: &str = "https://schemas.kicad.org/drc.v1.json";

/// Port of `class DRC_REPORT`: a board, the report units and the three marker providers of
/// `PCBNEW_JOBS_HANDLER::JobExportDrc` (DRC + drawing sheet, ratsnest, parity), with their
/// severity filters already applied.
pub struct DrcReport<'a> {
    ctx: &'a BoardCtx<'a>,
    report_units: EdaUnits,
    markers_provider: &'a DrcItemsProvider,
    ratsnest_provider: &'a DrcItemsProvider,
    fp_warnings_provider: &'a DrcItemsProvider,
    reported_severities: i32,
}

/// The `fn.GetFullName()` of the board file name (`wxFileName` on Unix: the part after the
/// last `/`).
fn file_full_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl<'a> DrcReport<'a> {
    /// `DRC_REPORT( aBoard, aReportUnits, aMarkersProvider, aRatsnestProvider,
    /// aFpWarningsProvider )`: the reported severities are those of the markers provider.
    pub fn new(
        ctx: &'a BoardCtx<'a>,
        report_units: EdaUnits,
        markers_provider: &'a DrcItemsProvider,
        ratsnest_provider: &'a DrcItemsProvider,
        fp_warnings_provider: &'a DrcItemsProvider,
    ) -> Self {
        DrcReport {
            ctx,
            report_units,
            markers_provider,
            ratsnest_provider,
            fp_warnings_provider,
            reported_severities: markers_provider.severities(),
        }
    }

    fn parent(&self, marker: &'a PcbMarker) -> Option<RcItemParent<'a>> {
        Some(RcItemParent {
            treated_as_excluded: marker.is_treated_as_excluded(self.ctx),
            comment: marker.comment(),
        })
    }

    /// The severity a violation of the markers provider is reported with: the marker's, but
    /// an exclusion shows the board severity of its code.
    fn marker_severity(&self, marker: &PcbMarker) -> Severity {
        let severity = marker.severity(self.ctx);

        if severity == Severity::Exclusion {
            self.ctx
                .board
                .design_settings()
                .get_severity(marker.rc_item().error_code())
        } else {
            severity
        }
    }

    /// Port of `DRC_REPORT::WriteTextReport`: the report text (written in the C locale).
    /// `date` replaces `GetISO8601CurrentDateTime()`.
    pub fn write_text_report(&self, date: &str) -> String {
        let board = self.ctx.board;
        let units = UnitsProvider::new(&PCB_IU_SCALE, self.report_units);
        let bds = board.design_settings();
        let item_resolver = BoardRcItemResolver::new(self.ctx);
        let mut out = String::new();

        out += &format!(
            "** Drc report for {} **\n",
            file_full_name(&board.file_name)
        );

        out += &format!("** Created on {date} **\n");

        out += &format!(
            "** Report includes: {} **\n",
            format_severities(self.reported_severities)
        );

        let count = self.markers_provider.count();

        out += &format!("\n** Found {count} DRC violations **\n");

        for &id in self.markers_provider.marker_ids() {
            let marker = &board[id];
            let severity = self.marker_severity(marker);

            out += &show_report(
                marker.rc_item(),
                self.parent(marker),
                &units,
                severity,
                &item_resolver,
            );
        }

        let count = self.ratsnest_provider.count();

        out += &format!("\n** Found {count} unconnected pads **\n");

        for &id in self.ratsnest_provider.marker_ids() {
            let marker = &board[id];
            let severity = bds.get_severity(marker.rc_item().error_code());

            out += &show_report(
                marker.rc_item(),
                self.parent(marker),
                &units,
                severity,
                &item_resolver,
            );
        }

        let count = self.fp_warnings_provider.count();

        out += &format!("\n** Found {count} Footprint errors **\n");

        for &id in self.fp_warnings_provider.marker_ids() {
            let marker = &board[id];
            let severity = bds.get_severity(marker.rc_item().error_code());

            out += &show_report(
                marker.rc_item(),
                self.parent(marker),
                &units,
                severity,
                &item_resolver,
            );
        }

        out += "\n** Ignored checks **\n";

        let mut has_ignored = false;

        for item in items_with_severities() {
            let code = item.code;

            if code > 0 && bds.ignore(code) {
                // GetErrorMessage( false ) of a static item is its title.
                out += &format!("    - {}\n", strip_markup(item.title));
                has_ignored = true;
            }
        }

        if !has_ignored {
            out += "    - None\n";
        }

        out += "\n** End of Report **\n";

        out
    }

    fn violations(&self, provider: &DrcItemsProvider, units: &UnitsProvider<'_>, board_severity: bool) -> Vec<JsonViolation> {
        let board = self.ctx.board;
        let bds = board.design_settings();
        let item_resolver = BoardRcItemResolver::new(self.ctx);

        provider
            .marker_ids()
            .iter()
            .map(|&id| {
                let marker = &board[id];
                let severity = if board_severity {
                    bds.get_severity(marker.rc_item().error_code())
                } else {
                    self.marker_severity(marker)
                };

                json_violation(
                    marker.rc_item(),
                    self.parent(marker),
                    units,
                    severity,
                    &item_resolver,
                )
            })
            .collect()
    }

    /// The `RC_JSON::DRC_REPORT` that `WriteJsonReport` serialises.
    pub fn json_report(&self, date: &str, kicad_version: &str) -> JsonDrcReport {
        let board = self.ctx.board;
        let units = UnitsProvider::new(&PCB_IU_SCALE, self.report_units);
        let bds = board.design_settings();

        let mut report_head = JsonDrcReport {
            schema: DRC_JSON_SCHEMA_URL.to_string(),
            source: file_full_name(&board.file_name).to_string(),
            date: date.to_string(),
            kicad_version: kicad_version.to_string(),
            coordinate_units: get_label(self.report_units, EdaDataType::Distance),
            ..JsonDrcReport::default()
        };

        // Document which severities are included in this report
        if self.reported_severities & Severity::Error as i32 != 0 {
            report_head.included_severities.push("error".to_string());
        }

        if self.reported_severities & Severity::Warning as i32 != 0 {
            report_head.included_severities.push("warning".to_string());
        }

        if self.reported_severities & Severity::Exclusion as i32 != 0 {
            report_head.included_severities.push("exclusion".to_string());
        }

        // One resolver per section is equivalent to the C++ single resolver: its miss cache
        // has no observable effect while the board does not change.
        report_head.violations = self.violations(self.markers_provider, &units, false);
        report_head.unconnected_items = self.violations(self.ratsnest_provider, &units, true);
        report_head.schematic_parity = self.violations(self.fp_warnings_provider, &units, true);

        for item in items_with_severities() {
            let code = item.code;

            if code > 0 && bds.ignore(code) {
                report_head.ignored_checks.push(JsonIgnoredCheck {
                    key: item.settings_key.to_string(),
                    description: strip_markup(item.title),
                });
            }
        }

        report_head
    }

    /// Port of `DRC_REPORT::WriteJsonReport`: `std::setw( 4 ) << nlohmann::json( reportHead )
    /// << std::endl`. `date` replaces `GetISO8601CurrentDateTime()`, `kicad_version`
    /// `GetMajorMinorPatchVersion()` (normally [`KICAD_MAJOR_MINOR_PATCH_VERSION`]).
    pub fn write_json_report(&self, date: &str, kicad_version: &str) -> String {
        let json = self.json_report(date, kicad_version).to_json();
        dump_stream(&json, 4, ObjectOrder::Sorted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_name() {
        assert_eq!(file_full_name("/tmp/x/board.kicad_pcb"), "board.kicad_pcb");
        assert_eq!(file_full_name("board.kicad_pcb"), "board.kicad_pcb");
        assert_eq!(file_full_name(""), "");
    }
}
