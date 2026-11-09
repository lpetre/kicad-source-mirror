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

//! Port of `pcbnew/teardrop/teardrop_parameters.h`, `teardrop_parameters.cpp` and
//! `pcbnew/teardrop/teardrop_types.h` (`board-model-types.md` section 1).

use kicad_core::PCB_IU_SCALE;

/// `TARGET_TD`: the kind of item a teardrop parameter set applies to (index into
/// `TeardropParametersList::params_list`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum TargetTd {
    Unknown = -1,
    Round = 0,
    Rect = 1,
    Track = 2,
    Count = 3,
}

/// `TEARDROP_TYPE` (`teardrop_types.h`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum TeardropType {
    /// Not a teardrop: just a standard zone.
    #[default]
    TdNone = 0,
    /// Not specified / unknown teardrop type.
    TdUnspecified,
    /// A teardrop on a via or pad.
    TdViapad,
    /// A teardrop on a track end (two tracks of different widths).
    TdTrackend,
}

/// `TEARDROP_PARAMETERS`. Equality is the C++ `operator==` (every member compared).
#[derive(Clone, Debug, PartialEq)]
pub struct TeardropParameters {
    /// Max allowed length for teardrops in IU; <= 0 to disable.
    pub td_max_len: i32, // m_TdMaxLen
    /// Max allowed height for teardrops in IU; <= 0 to disable.
    pub td_max_width: i32, // m_TdMaxWidth
    /// Length of a teardrop as ratio between length and size of pad / via.
    pub best_length_ratio: f64, // m_BestLengthRatio
    /// Height of a teardrop as ratio between height and size of pad / via.
    pub best_width_ratio: f64, // m_BestWidthRatio
    /// Ratio (H/D) between via / pad size and track width max value to create a teardrop.
    pub width_to_size_filter_ratio: f64, // m_WidthtoSizeFilterRatio
    pub curved_edges: bool,         // m_CurvedEdges
    pub enabled: bool,              // m_Enabled
    pub allow_use_two_tracks: bool, // m_AllowUseTwoTracks
    pub td_on_pads_in_zones: bool,  // m_TdOnPadsInZones
}

impl Default for TeardropParameters {
    /// `TEARDROP_PARAMETERS()`.
    fn default() -> Self {
        TeardropParameters {
            td_max_len: PCB_IU_SCALE.mm_to_iu(1.0),
            td_max_width: PCB_IU_SCALE.mm_to_iu(2.0),
            best_length_ratio: 0.5,
            best_width_ratio: 1.0,
            width_to_size_filter_ratio: 0.9,
            curved_edges: false,
            enabled: false,
            allow_use_two_tracks: true,
            td_on_pads_in_zones: false,
        }
    }
}

impl TeardropParameters {
    /// `TEARDROP_PARAMETERS::SetTeardropMaxSize`.
    pub fn set_teardrop_max_size(&mut self, max_len: i32, max_height: i32) {
        self.td_max_len = max_len;
        self.td_max_width = max_height;
    }

    /// `TEARDROP_PARAMETERS::SetTeardropSizeRatio` (C++ defaults 0.5, 1.0).
    pub fn set_teardrop_size_ratio(&mut self, length_ratio: f64, height_ratio: f64) {
        self.best_length_ratio = length_ratio;
        self.best_width_ratio = height_ratio;
    }
}

/// `TEARDROP_PARAMETERS_LIST`.
#[derive(Clone, Debug, PartialEq)]
pub struct TeardropParametersList {
    /// m_params_list: `[TARGET_ROUND, TARGET_RECT, TARGET_TRACK]`.
    pub params_list: Vec<TeardropParameters>,
    pub target_vias: bool,           // m_TargetVias
    pub target_pth_pads: bool,       // m_TargetPTHPads
    pub target_smd_pads: bool,       // m_TargetSMDPads
    pub target_track2track: bool,    // m_TargetTrack2Track
    pub use_round_shapes_only: bool, // m_UseRoundShapesOnly
}

impl Default for TeardropParametersList {
    /// `TEARDROP_PARAMETERS_LIST()`: one default parameter set per target.
    fn default() -> Self {
        TeardropParametersList {
            params_list: vec![
                TeardropParameters::default(), // parameters for TARGET_ROUND
                TeardropParameters::default(), // parameters for TARGET_RECT
                TeardropParameters::default(), // parameters for TARGET_TRACK
            ],
            target_vias: true,
            target_pth_pads: true,
            target_smd_pads: true,
            target_track2track: false,
            use_round_shapes_only: false,
        }
    }
}

impl TeardropParametersList {
    /// `TEARDROP_PARAMETERS_LIST::GetParameters( aTdType )` (`std::vector::at`: panics on
    /// `Unknown` / `Count`, where C++ throws).
    pub fn parameters(&self, td_type: TargetTd) -> &TeardropParameters {
        &self.params_list[td_type as i32 as usize]
    }

    /// Mutable `GetParameters( aTdType )`.
    pub fn parameters_mut(&mut self, td_type: TargetTd) -> &mut TeardropParameters {
        &mut self.params_list[td_type as i32 as usize]
    }

    /// `TEARDROP_PARAMETERS_LIST::GetParametersCount`.
    pub fn parameters_count(&self) -> usize {
        self.params_list.len()
    }
}

const TARGET_NAME_ROUND: &str = "td_round_shape";
const TARGET_NAME_RECT: &str = "td_rect_shape";
const TARGET_NAME_TRACK: &str = "td_track_end";

/// Port of `GetTeardropTargetCanonicalName`: the canonical name of a target (empty for the
/// others).
pub fn teardrop_target_canonical_name(td_type: TargetTd) -> String {
    match td_type {
        TargetTd::Round => TARGET_NAME_ROUND,
        TargetTd::Rect => TARGET_NAME_RECT,
        TargetTd::Track => TARGET_NAME_TRACK,
        _ => "",
    }
    .to_string()
}

/// Port of `GetTeardropTargetTypeFromCanonicalName`.
pub fn teardrop_target_type_from_canonical_name(target_name: &str) -> TargetTd {
    match target_name {
        TARGET_NAME_ROUND => TargetTd::Round,
        TARGET_NAME_RECT => TargetTd::Rect,
        TARGET_NAME_TRACK => TargetTd::Track,
        _ => TargetTd::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Defaults of `TEARDROP_PARAMETERS()` and `TEARDROP_PARAMETERS_LIST()`
    /// (`teardrop_parameters.h`).
    #[test]
    fn teardrop_defaults() {
        let p = TeardropParameters::default();
        assert_eq!(p.td_max_len, 1_000_000);
        assert_eq!(p.td_max_width, 2_000_000);
        assert_eq!(p.best_length_ratio, 0.5);
        assert_eq!(p.best_width_ratio, 1.0);
        assert_eq!(p.width_to_size_filter_ratio, 0.9);
        assert!(!p.curved_edges && !p.enabled && p.allow_use_two_tracks);
        assert!(!p.td_on_pads_in_zones);

        let l = TeardropParametersList::default();
        assert_eq!(l.parameters_count(), 3);
        assert_eq!(*l.parameters(TargetTd::Track), p);
        assert!(l.target_vias && l.target_pth_pads && l.target_smd_pads);
        assert!(!l.target_track2track && !l.use_round_shapes_only);
        assert_eq!(TeardropType::default(), TeardropType::TdNone);
    }

    #[test]
    fn teardrop_target_names_round_trip() {
        for t in [TargetTd::Round, TargetTd::Rect, TargetTd::Track] {
            let name = teardrop_target_canonical_name(t);
            assert_eq!(teardrop_target_type_from_canonical_name(&name), t);
        }
        assert_eq!(teardrop_target_canonical_name(TargetTd::Unknown), "");
        assert_eq!(
            teardrop_target_type_from_canonical_name("x"),
            TargetTd::Unknown
        );
    }
}
