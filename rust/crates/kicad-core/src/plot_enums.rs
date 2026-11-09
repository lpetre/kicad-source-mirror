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

//! Port of the plot enums shared by the board model and the plot engines:
//! `PLOT_FORMAT`, `PLOT_TEXT_MODE`, `DXF_OUTLINE_MODE`, `DXF_UNITS`, `DXF_LAYER_OUTPUT_MODE`
//! (`include/plotters/plotter.h`), `DRILL_MARKS` (`pcbnew/plotprint_opts.h`) and `FILL_T`
//! (`include/eda_fill.h`).
//!
//! They live in `kicad-core` because `PCB_PLOT_PARAMS` (board model) stores them and the plot
//! engines (`kicad-plot`) consume them, and neither crate depends on the other. The dash style
//! of the plotters is [`crate::stroke_params::LineStyle`] (`LINE_STYLE`).

/// Port of `enum class PLOT_FORMAT`: the set of supported output plot formats.
///
/// They should be kept in order of the radio buttons in the plot panel/windows; the integer
/// values are the `outputformat` token of `(pcbplotparams)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum PlotFormat {
    /// `UNDEFINED`
    Undefined = -1,
    /// `HPGL` (`FIRST_FORMAT`)
    Hpgl = 0,
    /// `GERBER`
    #[default]
    Gerber = 1,
    /// `POST`
    Post = 2,
    /// `DXF`
    Dxf = 3,
    /// `PDF`
    Pdf = 4,
    /// `SVG`
    Svg = 5,
    /// `PNG` (`LAST_FORMAT`)
    Png = 6,
}

impl PlotFormat {
    /// `PLOT_FORMAT::FIRST_FORMAT`.
    pub const FIRST_FORMAT: PlotFormat = PlotFormat::Hpgl;
    /// `PLOT_FORMAT::LAST_FORMAT`.
    pub const LAST_FORMAT: PlotFormat = PlotFormat::Png;

    /// The C++ integer value (`static_cast<int>`).
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// The enum value of a C++ integer (`static_cast<PLOT_FORMAT>`); `None` when out of range.
    pub const fn from_i32(value: i32) -> Option<PlotFormat> {
        Some(match value {
            -1 => PlotFormat::Undefined,
            0 => PlotFormat::Hpgl,
            1 => PlotFormat::Gerber,
            2 => PlotFormat::Post,
            3 => PlotFormat::Dxf,
            4 => PlotFormat::Pdf,
            5 => PlotFormat::Svg,
            6 => PlotFormat::Png,
            _ => return None,
        })
    }
}

/// Port of `enum class PLOT_TEXT_MODE`: which kind of text to output with the PSLIKE plotters.
///
/// 1) only use the internal vector font, 2) only use native postscript fonts, 3) use the
/// internal vector font and add 'phantom' text to aid searching, 4) keep the default for the
/// plot driver. This is recognized by the DXF driver too, where `NATIVE` emits TEXT entities
/// instead of stroking the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum PlotTextMode {
    /// `STROKE`
    Stroke = 0,
    /// `NATIVE`
    Native = 1,
    /// `PHANTOM`
    Phantom = 2,
    /// `DEFAULT`
    #[default]
    Default = 3,
}

/// Port of `enum DXF_OUTLINE_MODE`: options to draw items with thickness (segments, arcs,
/// circles, texts...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum DxfOutlineMode {
    /// `SKETCH`: sketch mode, draw segments outlines only
    Sketch = 0,
    /// `FILLED`: normal mode, solid segments
    #[default]
    Filled = 1,
}

/// Port of `enum class DXF_UNITS`: units (inch/mm) for the DXF plotter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum DxfUnits {
    /// `INCH` (do not use MM: it conflicts with a Windows header)
    #[default]
    Inch = 0,
    /// `MM`
    Mm = 1,
}

/// Port of `enum class DXF_LAYER_OUTPUT_MODE`: whether the DXF layer gets the layer name or
/// the colour name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum DxfLayerOutputMode {
    /// `Layer_Name`
    #[default]
    LayerName = 0,
    /// `Layer_Color_Name`
    LayerColorName = 1,
    /// `Current_Layer_Name`
    CurrentLayerName = 2,
    /// `Current_Layer_Color_Name`
    CurrentLayerColorName = 3,
}

/// Port of `enum class DRILL_MARKS` (`pcbnew/plotprint_opts.h`): plot no holes, small holes
/// with a fixed size, or holes using the actual size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum DrillMarks {
    /// `NO_DRILL_SHAPE`
    NoDrillShape = 0,
    /// `SMALL_DRILL_SHAPE` (the `PCB_PLOT_PARAMS` default)
    #[default]
    SmallDrillShape = 1,
    /// `FULL_DRILL_SHAPE`
    FullDrillShape = 2,
}

impl DrillMarks {
    /// The enum value of the `drillshape` token value; `None` when out of range.
    pub const fn from_i32(value: i32) -> Option<DrillMarks> {
        Some(match value {
            0 => DrillMarks::NoDrillShape,
            1 => DrillMarks::SmallDrillShape,
            2 => DrillMarks::FullDrillShape,
            _ => return None,
        })
    }
}

/// Port of `enum class FILL_T` (`include/eda_fill.h`).
///
/// Do not change these values without updating dialogs that depend on their position values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum FillT {
    /// `NO_FILL`
    #[default]
    NoFill = 1,
    /// `FILLED_SHAPE`: fill with object color.
    FilledShape = 2,
    /// `FILLED_WITH_BG_BODYCOLOR`: fill with background body color.
    FilledWithBgBodycolor = 3,
    /// `FILLED_WITH_COLOR`: fill with a separate color.
    FilledWithColor = 4,
    /// `HATCH`
    Hatch = 5,
    /// `REVERSE_HATCH`
    ReverseHatch = 6,
    /// `CROSS_HATCH`
    CrossHatch = 7,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_match_cpp() {
        assert_eq!(PlotFormat::Gerber as i32, 1);
        assert_eq!(PlotFormat::Svg as i32, 5);
        assert_eq!(PlotFormat::from_i32(6), Some(PlotFormat::Png));
        assert_eq!(PlotFormat::from_i32(7), None);
        assert_eq!(FillT::NoFill as i32, 1);
        assert_eq!(FillT::CrossHatch as i32, 7);
        assert_eq!(DrillMarks::from_i32(2), Some(DrillMarks::FullDrillShape));
        assert_eq!(PlotTextMode::Default as i32, 3);
    }
}
