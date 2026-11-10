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

//! Port of the part of `common/settings/color_settings.cpp` / `include/settings/color_settings.h`
//! (`COLOR_SETTINGS`), `common/settings/builtin_color_themes.h` (`s_defaultTheme`,
//! `s_classicTheme`) and `SETTINGS_MANAGER::GetColorSettings` that the board plot reads
//! (work package FAB-11): `PCB_PLOT_PARAMS::ColorSettings()->GetColor( aLayer )`.
//!
//! Subset: the two built-in themes, restricted to the keys the board plot can ask for (the
//! board layers `F_Cu` .. `User_45`, `LAYER_DNP_MARKER`, `LAYER_PCB_BACKGROUND`,
//! `LAYER_DRAWINGSHEET`); every other key answers `UNSPECIFIED` as a key without a parameter
//! does in C++ (e.g. `LAYER_VIAS + viatype`, which neither built-in theme defines). Theme files
//! (`<config>/colors/<name>.json`) cannot be read by the port: a name that is not a built-in
//! theme behaves as in C++ when no such file exists (a copy of the default theme named `user`).
//! The port's `PcbPlotParams` has no colour settings (dropped by the model); the plot
//! functions take a [`ColorSettings`] from their caller instead.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use kicad_core::PcbLayerId;
use kicad_core::color4d::{Color4D, EdaColorT};
use kicad_core::layer_ids::{LAYER_DNP_MARKER, LAYER_DRAWINGSHEET, LAYER_PCB_BACKGROUND};

/// `COLOR_SETTINGS::COLOR_BUILTIN_DEFAULT`
pub const COLOR_BUILTIN_DEFAULT: &str = "_builtin_default";
/// `COLOR_SETTINGS::COLOR_BUILTIN_CLASSIC`
pub const COLOR_BUILTIN_CLASSIC: &str = "_builtin_classic";
/// `DEFAULT_THEME` (`settings_manager.cpp`): the file name of the user theme.
pub const DEFAULT_THEME: &str = "user";

/// Which colour table a [`ColorSettings`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorTable {
    /// `m_colors` empty: a `COLOR_SETTINGS` that was constructed but never loaded (the
    /// `m_default_colors` of a default-constructed `PCB_PLOT_PARAMS`).
    Empty,
    /// The parameter defaults (`s_defaultTheme`): `_builtin_default` and every theme loaded
    /// from defaults (`user`, unknown names).
    Default,
    /// `s_classicTheme` (`_builtin_classic`).
    Classic,
}

/// Port of the read-only part of `COLOR_SETTINGS` the board plot uses: the file name (to
/// detect a missing theme) and `GetColor`. Cheap to copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorSettings {
    /// `GetFilename()`
    filename: &'static str,
    table: ColorTable,
}

impl Default for ColorSettings {
    /// The `m_default_colors` of `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS()`: `new COLOR_SETTINGS()`
    /// (file name `user`), never loaded, so every `GetColor` is `UNSPECIFIED`.
    fn default() -> Self {
        ColorSettings {
            filename: DEFAULT_THEME,
            table: ColorTable::Empty,
        }
    }
}

impl ColorSettings {
    /// The `_builtin_default` theme of `COLOR_SETTINGS::CreateBuiltinColorSettings`
    /// (`Load()` from the parameter defaults).
    pub fn builtin_default() -> Self {
        ColorSettings {
            filename: COLOR_BUILTIN_DEFAULT,
            table: ColorTable::Default,
        }
    }

    /// The `_builtin_classic` theme of `COLOR_SETTINGS::CreateBuiltinColorSettings`
    /// (`m_colors` = `s_classicTheme`).
    pub fn builtin_classic() -> Self {
        ColorSettings {
            filename: COLOR_BUILTIN_CLASSIC,
            table: ColorTable::Classic,
        }
    }

    /// `JSON_SETTINGS::GetFilename()`.
    pub fn filename(&self) -> &'static str {
        self.filename
    }

    /// Port of `COLOR_SETTINGS::GetColor( int aLayer )`: the colour of a layer, `UNSPECIFIED`
    /// for a key the theme does not hold.
    pub fn get_color(&self, a_layer: i32) -> Color4D {
        let map = match self.table {
            ColorTable::Empty => return Color4D::UNSPECIFIED,
            ColorTable::Default => default_theme(),
            ColorTable::Classic => classic_theme(),
        };

        match map.get(&a_layer) {
            Some(color) => color.clone(),
            None => Color4D::UNSPECIFIED,
        }
    }
}

/// Port of `SETTINGS_MANAGER::GetColorSettings( aName )` for a fresh configuration (the
/// built-in themes and the `user` theme, no theme files): by key, then by display name
/// (case-insensitive), else a copy of the default theme with the file name `user`
/// (`registerColorSettings` after a failed `loadColorSettingsByName`). An empty name returns
/// the default theme.
pub fn get_color_settings(a_name: &str) -> ColorSettings {
    match a_name {
        COLOR_BUILTIN_DEFAULT => return ColorSettings::builtin_default(),
        COLOR_BUILTIN_CLASSIC => return ColorSettings::builtin_classic(),
        DEFAULT_THEME => {
            // The `user` theme: a COLOR_SETTINGS loaded from its parameter defaults
            return ColorSettings {
                filename: DEFAULT_THEME,
                table: ColorTable::Default,
            };
        }
        _ => {}
    }

    // Maybe it's the display name (cli is one method of invoke). `m_color_settings` is a
    // std::map ordered by key: "_builtin_classic", "_builtin_default", "user"; the user theme's
    // display name is "KiCad Default" too (`meta.name` default), so it never wins.
    let lower = a_name.to_lowercase();

    if lower == "kicad classic" {
        return ColorSettings::builtin_classic();
    }

    if lower == "kicad default" {
        return ColorSettings::builtin_default();
    }

    if a_name.is_empty() {
        // This had better work
        return ColorSettings::builtin_default();
    }

    // No match: *ret = *builtin default; ret->SetFilename( DEFAULT_THEME )
    ColorSettings {
        filename: DEFAULT_THEME,
        table: ColorTable::Default,
    }
}

/// `CSS_COLOR( r, g, b, a )`: `COLOR4D().FromCSSRGBA( r, g, b, a )`.
fn css(r: i32, g: i32, b: i32, a: f64) -> Color4D {
    let mut color = Color4D::UNSPECIFIED;
    color.from_css_rgba(r, g, b, a);
    color
}

/// `COLOR4D( EDA_COLOR_T )`.
fn eda(c: EdaColorT) -> Color4D {
    Color4D::from_eda_color(c)
}

/// The board part of `s_defaultTheme` (keys with a `COLOR_MAP_PARAM`).
fn default_theme() -> &'static BTreeMap<i32, Color4D> {
    static MAP: OnceLock<BTreeMap<i32, Color4D>> = OnceLock::new();

    MAP.get_or_init(|| {
        [
            (LAYER_DNP_MARKER, css(220, 9, 13, 0.85)),
            (LAYER_PCB_BACKGROUND, css(0, 16, 35, 1.0)),
            (LAYER_DRAWINGSHEET, css(200, 114, 171, 1.0)),
            (PcbLayerId::FCu.0, css(200, 52, 52, 1.0)),
            (PcbLayerId::In1Cu.0, css(127, 200, 127, 1.0)),
            (PcbLayerId::In2Cu.0, css(206, 125, 44, 1.0)),
            (PcbLayerId::In3Cu.0, css(79, 203, 203, 1.0)),
            (PcbLayerId::In4Cu.0, css(219, 98, 139, 1.0)),
            (PcbLayerId::In5Cu.0, css(167, 165, 198, 1.0)),
            (PcbLayerId::In6Cu.0, css(40, 204, 217, 1.0)),
            (PcbLayerId::In7Cu.0, css(232, 178, 167, 1.0)),
            (PcbLayerId::In8Cu.0, css(242, 237, 161, 1.0)),
            (PcbLayerId::In9Cu.0, css(141, 203, 129, 1.0)),
            (PcbLayerId::In10Cu.0, css(237, 124, 51, 1.0)),
            (PcbLayerId::In11Cu.0, css(91, 195, 235, 1.0)),
            (PcbLayerId::In12Cu.0, css(247, 111, 142, 1.0)),
            (PcbLayerId::In13Cu.0, css(167, 165, 198, 1.0)),
            (PcbLayerId::In14Cu.0, css(40, 204, 217, 1.0)),
            (PcbLayerId::In15Cu.0, css(232, 178, 167, 1.0)),
            (PcbLayerId::In16Cu.0, css(242, 237, 161, 1.0)),
            (PcbLayerId::In17Cu.0, css(237, 124, 51, 1.0)),
            (PcbLayerId::In18Cu.0, css(91, 195, 235, 1.0)),
            (PcbLayerId::In19Cu.0, css(247, 111, 142, 1.0)),
            (PcbLayerId::In20Cu.0, css(167, 165, 198, 1.0)),
            (PcbLayerId::In21Cu.0, css(40, 204, 217, 1.0)),
            (PcbLayerId::In22Cu.0, css(232, 178, 167, 1.0)),
            (PcbLayerId::In23Cu.0, css(242, 237, 161, 1.0)),
            (PcbLayerId::In24Cu.0, css(237, 124, 51, 1.0)),
            (PcbLayerId::In25Cu.0, css(91, 195, 235, 1.0)),
            (PcbLayerId::In26Cu.0, css(247, 111, 142, 1.0)),
            (PcbLayerId::In27Cu.0, css(167, 165, 198, 1.0)),
            (PcbLayerId::In28Cu.0, css(40, 204, 217, 1.0)),
            (PcbLayerId::In29Cu.0, css(232, 178, 167, 1.0)),
            (PcbLayerId::In30Cu.0, css(242, 237, 161, 1.0)),
            (PcbLayerId::BCu.0, css(77, 127, 196, 1.0)),
            (PcbLayerId::BAdhes.0, css(0, 0, 132, 1.0)),
            (PcbLayerId::FAdhes.0, css(132, 0, 132, 1.0)),
            (PcbLayerId::BPaste.0, css(0, 194, 194, 0.9)),
            (PcbLayerId::FPaste.0, css(180, 160, 154, 0.9)),
            (PcbLayerId::BSilkS.0, css(232, 178, 167, 1.0)),
            (PcbLayerId::FSilkS.0, css(242, 237, 161, 1.0)),
            (PcbLayerId::BMask.0, css(2, 255, 238, 0.4)),
            (PcbLayerId::FMask.0, css(216, 100, 255, 0.4)),
            (PcbLayerId::DwgsUser.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::CmtsUser.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::Eco1User.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::Eco2User.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::EdgeCuts.0, css(208, 210, 205, 1.0)),
            (PcbLayerId::Margin.0, css(255, 38, 226, 1.0)),
            (PcbLayerId::BCrtYd.0, css(38, 233, 255, 1.0)),
            (PcbLayerId::FCrtYd.0, css(255, 38, 226, 1.0)),
            (PcbLayerId::BFab.0, css(88, 93, 132, 1.0)),
            (PcbLayerId::FFab.0, css(175, 175, 175, 1.0)),
            (PcbLayerId::User1.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User2.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User3.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User4.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User5.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User6.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User7.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User8.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User9.0, css(232, 178, 167, 1.0)),
            (PcbLayerId::User10.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User11.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User12.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User13.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User14.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User15.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User16.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User17.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User18.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User19.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User20.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User21.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User22.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User23.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User24.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User25.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User26.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User27.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User28.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User29.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User30.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User31.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User32.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User33.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User34.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User35.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User36.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User37.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User38.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User39.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User40.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User41.0, css(194, 194, 194, 1.0)),
            (PcbLayerId::User42.0, css(89, 148, 220, 1.0)),
            (PcbLayerId::User43.0, css(180, 219, 210, 1.0)),
            (PcbLayerId::User44.0, css(216, 200, 82, 1.0)),
            (PcbLayerId::User45.0, css(194, 194, 194, 1.0)),
        ]
        .into_iter()
        .collect()
    })
}

/// The board part of `s_classicTheme`.
fn classic_theme() -> &'static BTreeMap<i32, Color4D> {
    static MAP: OnceLock<BTreeMap<i32, Color4D>> = OnceLock::new();

    MAP.get_or_init(|| {
        [
            (LAYER_DNP_MARKER, eda(EdaColorT::PURERED).with_alpha(0.85)),
            (LAYER_PCB_BACKGROUND, eda(EdaColorT::BLACK)),
            (LAYER_DRAWINGSHEET, eda(EdaColorT::DARKRED)),
            (PcbLayerId::FCu.0, eda(EdaColorT::RED)),
            (PcbLayerId::In1Cu.0, eda(EdaColorT::YELLOW)),
            (PcbLayerId::In2Cu.0, eda(EdaColorT::LIGHTMAGENTA)),
            (PcbLayerId::In3Cu.0, eda(EdaColorT::LIGHTRED)),
            (PcbLayerId::In4Cu.0, eda(EdaColorT::CYAN)),
            (PcbLayerId::In5Cu.0, eda(EdaColorT::GREEN)),
            (PcbLayerId::In6Cu.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::In7Cu.0, eda(EdaColorT::DARKGRAY)),
            (PcbLayerId::In8Cu.0, eda(EdaColorT::MAGENTA)),
            (PcbLayerId::In9Cu.0, eda(EdaColorT::LIGHTGRAY)),
            (PcbLayerId::In10Cu.0, eda(EdaColorT::MAGENTA)),
            (PcbLayerId::In11Cu.0, eda(EdaColorT::RED)),
            (PcbLayerId::In12Cu.0, eda(EdaColorT::BROWN)),
            (PcbLayerId::In13Cu.0, eda(EdaColorT::LIGHTGRAY)),
            (PcbLayerId::In14Cu.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::In15Cu.0, eda(EdaColorT::GREEN)),
            (PcbLayerId::In16Cu.0, eda(EdaColorT::RED)),
            (PcbLayerId::In17Cu.0, eda(EdaColorT::YELLOW)),
            (PcbLayerId::In18Cu.0, eda(EdaColorT::LIGHTMAGENTA)),
            (PcbLayerId::In19Cu.0, eda(EdaColorT::LIGHTRED)),
            (PcbLayerId::In20Cu.0, eda(EdaColorT::CYAN)),
            (PcbLayerId::In21Cu.0, eda(EdaColorT::GREEN)),
            (PcbLayerId::In22Cu.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::In23Cu.0, eda(EdaColorT::DARKGRAY)),
            (PcbLayerId::In24Cu.0, eda(EdaColorT::MAGENTA)),
            (PcbLayerId::In25Cu.0, eda(EdaColorT::LIGHTGRAY)),
            (PcbLayerId::In26Cu.0, eda(EdaColorT::MAGENTA)),
            (PcbLayerId::In27Cu.0, eda(EdaColorT::RED)),
            (PcbLayerId::In28Cu.0, eda(EdaColorT::BROWN)),
            (PcbLayerId::In29Cu.0, eda(EdaColorT::LIGHTGRAY)),
            (PcbLayerId::In30Cu.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::BCu.0, eda(EdaColorT::GREEN)),
            (PcbLayerId::BAdhes.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::FAdhes.0, eda(EdaColorT::MAGENTA)),
            (PcbLayerId::BPaste.0, eda(EdaColorT::LIGHTCYAN)),
            (PcbLayerId::FPaste.0, eda(EdaColorT::RED)),
            (PcbLayerId::BSilkS.0, eda(EdaColorT::MAGENTA)),
            (PcbLayerId::FSilkS.0, eda(EdaColorT::CYAN)),
            (PcbLayerId::BMask.0, eda(EdaColorT::BROWN)),
            (PcbLayerId::FMask.0, eda(EdaColorT::MAGENTA)),
            (PcbLayerId::DwgsUser.0, eda(EdaColorT::LIGHTGRAY)),
            (PcbLayerId::CmtsUser.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::Eco1User.0, eda(EdaColorT::GREEN)),
            (PcbLayerId::Eco2User.0, eda(EdaColorT::YELLOW)),
            (PcbLayerId::EdgeCuts.0, eda(EdaColorT::YELLOW)),
            (PcbLayerId::Margin.0, eda(EdaColorT::LIGHTMAGENTA)),
            (PcbLayerId::BCrtYd.0, eda(EdaColorT::DARKGRAY)),
            (PcbLayerId::FCrtYd.0, eda(EdaColorT::LIGHTGRAY)),
            (PcbLayerId::BFab.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::FFab.0, eda(EdaColorT::DARKGRAY)),
            (PcbLayerId::User1.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User2.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User3.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User4.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User5.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User6.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User7.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User8.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User9.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User10.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User11.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User12.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User13.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User14.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User15.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User16.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User17.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User18.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User19.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User20.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User21.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User22.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User23.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User24.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User25.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User26.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User27.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User28.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User29.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User30.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User31.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User32.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User33.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User34.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User35.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User36.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User37.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User38.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User39.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User40.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User41.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User42.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User43.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User44.0, eda(EdaColorT::BLUE)),
            (PcbLayerId::User45.0, eda(EdaColorT::BLUE)),
        ]
        .into_iter()
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_lookup() {
        assert_eq!(get_color_settings("").filename(), COLOR_BUILTIN_DEFAULT);
        assert_eq!(
            get_color_settings("KiCad Classic"),
            ColorSettings::builtin_classic()
        );
        assert_eq!(get_color_settings("nope").filename(), DEFAULT_THEME);
        assert_eq!(
            get_color_settings("nope").get_color(PcbLayerId::FCu.0),
            css(200, 52, 52, 1.0)
        );
        assert_eq!(
            ColorSettings::default().get_color(PcbLayerId::FCu.0),
            Color4D::UNSPECIFIED
        );
        assert_eq!(
            ColorSettings::builtin_classic().get_color(PcbLayerId::BCu.0),
            eda(EdaColorT::GREEN)
        );
        // LAYER_VIAS + viatype has no parameter
        assert_eq!(
            ColorSettings::builtin_default().get_color(kicad_core::layer_ids::LAYER_VIAS + 3),
            Color4D::UNSPECIFIED
        );
        assert_eq!(default_theme().len(), 98);
        assert_eq!(classic_theme().len(), 98);
    }
}
