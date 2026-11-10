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

//! Port of `common/project/project_local_settings.cpp` (`PROJECT_LOCAL_SETTINGS`, the
//! `.kicad_prl` document), restricted to what the board side reads headlessly
//! (`docs/analysis/settings.md` 3.11): `board.visible_layers` and
//! `board.prototype_zone_fills`. Package SET-5.
//!
//! Not ported: `board.visible_items` and its schema migrations 1->2 .. 6->7 (they only rewrite
//! `visible_items`; package SET-9), and every other (GUI) member.

use kicad_core::Lset;
use kicad_core::json_settings::JsonSettings;
use kicad_core::settings_field;
use kicad_core::settings_parameters::{ParamLambda, ParamT};

/// `projectLocalSettingsVersion` (project_local_settings.cpp:29).
pub const PROJECT_LOCAL_SETTINGS_SCHEMA_VERSION: i32 = 7;

/// Port of the ported members of `PROJECT_LOCAL_SETTINGS`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectLocalSettings {
    /// `m_VisibleLayers` (`board.visible_layers`, `LSET::FmtHex` text).
    pub visible_layers: Lset,
    /// `m_PrototypeZoneFill` (`board.prototype_zone_fills`).
    pub prototype_zone_fill: bool,
    /// `JSON_SETTINGS::LoadFromFile`'s result (`m_fileSynced`).
    pub file_synced: bool,
}

impl Default for ProjectLocalSettings {
    /// `PROJECT_LOCAL_SETTINGS` loaded without a file: every layer visible.
    fn default() -> Self {
        Self::load_from_str(None)
    }
}

impl ProjectLocalSettings {
    /// Port of `PROJECT_LOCAL_SETTINGS::LoadFromFile` (`JSON_SETTINGS::LoadFromFile`) on the
    /// text of the `.kicad_prl`; `None` = no file. A text that does not parse counts as no
    /// file (the C++ legacy fallback, `MigrateFromLegacy`, reads nothing).
    pub fn load_from_str(json_text: Option<&str>) -> ProjectLocalSettings {
        // The constructor's member initialisers: m_VisibleLayers is a default (empty) LSET,
        // m_PrototypeZoneFill( false ).
        let mut target = ProjectLocalSettings {
            visible_layers: Lset::new(),
            prototype_zone_fill: false,
            file_synced: false,
        };

        let mut settings: JsonSettings<ProjectLocalSettings> =
            JsonSettings::new(PROJECT_LOCAL_SETTINGS_SCHEMA_VERSION, None);

        settings.add_param(Box::new(ParamLambda::new(
            "board.visible_layers",
            |s: &ProjectLocalSettings| s.visible_layers.fmt_hex(),
            |s: &mut ProjectLocalSettings, value: String| {
                s.visible_layers.parse_hex(&value);
            },
            Lset::all_layers_mask().fmt_hex(),
        )));

        settings.add_param(Box::new(ParamT::new(
            "board.prototype_zone_fills",
            settings_field!(ProjectLocalSettings, prototype_zone_fill),
            false,
        )));

        let success = match json_text {
            Some(text) => settings.load_from_buffer(text.as_bytes(), &mut target),
            None => {
                settings.internals.clear();
                settings.load(&mut target);
                settings.load(&mut target);
                false
            }
        };

        target.file_synced = success;
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_core::PcbLayerId;

    #[test]
    fn no_file_every_layer_visible() {
        let s = ProjectLocalSettings::load_from_str(None);
        assert_eq!(&s.visible_layers, Lset::all_layers_mask());
        assert!(!s.prototype_zone_fill);
        assert!(!s.file_synced);
    }

    /// `LSET::ParseHex` reads nibbles from the end; the V9 numbering is applied unmigrated.
    #[test]
    fn visible_layers_parse_hex() {
        let s = ProjectLocalSettings::load_from_str(Some(
            r#"{ "meta": { "version": 7 },
                 "board": { "visible_layers": "00000000_00000000_00000000_00000005",
                            "prototype_zone_fills": true } }"#,
        ));
        assert!(s.file_synced);
        assert!(s.prototype_zone_fill);
        assert_eq!(s.visible_layers.count(), 2);
        assert!(s.visible_layers.contains(PcbLayerId::FCu));

        // Not a string: the lambda's default (all layers).
        let s = ProjectLocalSettings::load_from_str(Some(
            r#"{ "meta": { "version": 7 }, "board": { "visible_layers": 5 } }"#,
        ));
        assert_eq!(&s.visible_layers, Lset::all_layers_mask());

        // A terminating non-hex byte counts as consumed in BASE_SET::ParseHex
        // (`*rstart--` before the break), so the (empty) parsed set replaces the layers.
        let s = ProjectLocalSettings::load_from_str(Some(
            r#"{ "meta": { "version": 7 }, "board": { "visible_layers": "xyz" } }"#,
        ));
        assert_eq!(s.visible_layers.count(), 0);

        // An empty string consumes nothing: the set of the first Load() pass (all layers).
        let s = ProjectLocalSettings::load_from_str(Some(
            r#"{ "meta": { "version": 7 }, "board": { "visible_layers": "" } }"#,
        ));
        assert_eq!(&s.visible_layers, Lset::all_layers_mask());
    }
}
