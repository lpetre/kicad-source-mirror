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

//! Port of `pcbnew/drc/drc_length_report.h`: the per-net length report of the matched-length
//! provider.
//!
//! `ENTRY::items` (`std::set<BOARD_CONNECTED_ITEM*>`, address order) is a `BTreeSet<ItemId>`;
//! `netinfo` is the net code (the net is looked up on the board); `matchingRule` is the
//! rule's `DrcRuleInfo` (its `rule_index` is the identity of the `DRC_RULE*`).

use std::collections::BTreeSet;

use kicad_pcb::ItemId;
use kicad_pcb::drc_api::DrcRuleInfo;

/// Port of `DRC_LENGTH_REPORT::ENTRY`: the length statistics of one net under one rule.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LengthReportEntry {
    /// `netcode`
    pub netcode: i32,
    /// `netname`
    pub netname: String,
    /// `netinfo` (`None`: no such net)
    pub netinfo: Option<i32>,
    /// `fromItem`
    pub from_item: Option<ItemId>,
    /// `toItem`
    pub to_item: Option<ItemId>,
    /// `matchingRule`
    pub matching_rule: Option<DrcRuleInfo>,
    /// `from`
    pub from: String,
    /// `to`
    pub to: String,
    /// `items`
    pub items: BTreeSet<ItemId>,
    /// `viaCount`
    pub via_count: i32,
    /// `totalRoute`
    pub total_route: f64,
    /// `totalRouteDelay`
    pub total_route_delay: f64,
    /// `totalVia`
    pub total_via: i32,
    /// `totalViaDelay`
    pub total_via_delay: i64,
    /// `totalPadToDie`
    pub total_pad_to_die: i32,
    /// `totalPadToDieDelay`
    pub total_pad_to_die_delay: i64,
    /// `total`
    pub total: f64,
    /// `totalDelay`
    pub total_delay: f64,
}

/// Port of `DRC_LENGTH_REPORT`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrcLengthReport {
    /// `m_report`
    report: Vec<LengthReportEntry>,
}

impl DrcLengthReport {
    /// `DRC_LENGTH_REPORT()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `Clear()`.
    pub fn clear(&mut self) {
        self.report.clear();
    }

    /// `Add( ent )`.
    pub fn add(&mut self, ent: &LengthReportEntry) {
        self.report.push(ent.clone());
    }

    /// `GetEntries()`.
    pub fn entries(&self) -> &[LengthReportEntry] {
        &self.report
    }
}
