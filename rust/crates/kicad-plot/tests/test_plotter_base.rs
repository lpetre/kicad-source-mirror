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

//! Unit tests of the `PLOTTER` base class port (PLOT-01) with a recording plotter.
//!
//! `qa/tests/common/test_gerber_plotter.cpp` (`GerberPlotter/ZeroLengthSegmentEmitsSingleFlash`,
//! issue 24131) needs the Gerber engine and is ported with PLOT-03; the base-class half of it
//! (how `PLOTTER::ThickSegment` resolves the width sentinels of a zero-length segment) is
//! `zero_length_segment_sentinels` below.

use kicad_core::color4d::Color4D;
use kicad_core::stroke_params::LineStyle;
use kicad_geom::{ANGLE_0, ANGLE_90, EdaAngle, ShapePolySet, Vector2D, Vector2I};
use kicad_plot::plotter::*;
use kicad_plot::{FillT, PlotFormat};

#[derive(Default)]
struct Rec {
    state: PlotterState,
    log: Vec<String>,
}

impl Plotter for Rec {
    fn state(&self) -> &PlotterState {
        &self.state
    }
    fn state_mut(&mut self) -> &mut PlotterState {
        &mut self.state
    }
    fn plotter_type(&self) -> PlotFormat {
        PlotFormat::Undefined
    }
    fn start_plot(&mut self, _p: &str) -> bool {
        true
    }
    fn end_plot(&mut self) -> bool {
        true
    }
    fn set_current_line_width(&mut self, width: i32, _d: PlotData) {
        self.log.push(format!("W{width}"));
        if width == USE_DEFAULT_LINE_WIDTH {
            self.state.current_pen_width = 150;
        } else if width != DO_NOT_SET_LINE_WIDTH {
            self.state.current_pen_width = width;
        }
    }
    fn set_color(&mut self, _c: &Color4D) {}
    fn set_dash(&mut self, _w: i32, _s: LineStyle) {}
    fn set_viewport(&mut self, _o: Vector2I, _i: f64, _s: f64, _m: bool) {}
    fn rect(&mut self, p1: Vector2I, p2: Vector2I, _f: FillT, w: i32, _r: i32) {
        self.log
            .push(format!("R{},{} {},{} {w}", p1.x, p1.y, p2.x, p2.y));
    }
    fn circle(&mut self, pos: Vector2I, d: i32, fill: FillT, w: i32) {
        self.log
            .push(format!("C{},{} {d} {} {w}", pos.x, pos.y, fill as i32));
    }
    fn pen_to(&mut self, pos: Vector2I, plume: char) {
        self.log.push(format!("{plume}{},{}", pos.x, pos.y));
    }
    fn plot_poly(&mut self, c: &[Vector2I], _f: FillT, w: i32, _d: PlotData) {
        self.log.push(format!("Y{} {w}", c.len()));
    }
    fn flash_pad_circle(&mut self, _p: Vector2I, _d: i32, _a: PlotData) {}
    fn flash_pad_oval(&mut self, _p: Vector2I, _s: Vector2I, _o: EdaAngle, _a: PlotData) {}
    fn flash_pad_rect(&mut self, _p: Vector2I, _s: Vector2I, _o: EdaAngle, _a: PlotData) {}
    fn flash_pad_round_rect(
        &mut self,
        _p: Vector2I,
        _s: Vector2I,
        _r: i32,
        _o: EdaAngle,
        _a: PlotData,
    ) {
    }
    fn flash_pad_custom(
        &mut self,
        _p: Vector2I,
        _s: Vector2I,
        _o: EdaAngle,
        _ps: &ShapePolySet,
        _a: PlotData,
    ) {
    }
    fn flash_pad_trapez(&mut self, _p: Vector2I, _c: &[Vector2I; 4], _o: EdaAngle, _a: PlotData) {}
    fn flash_regular_polygon(
        &mut self,
        _p: Vector2I,
        _d: i32,
        _c: i32,
        _o: EdaAngle,
        _a: PlotData,
    ) {
    }
}

/// Issue 24131: a zero-length thick segment is a filled circle whose diameter is the
/// resolved pen width, never a sentinel.
#[test]
fn zero_length_segment_sentinels() {
    let o = Vector2I::new(0, 0);

    let mut p = Rec::default();
    p.thick_segment(o, o, 2000000, None);
    assert_eq!(p.log, ["C0,0 2000000 2 0"]);

    let mut p = Rec::default();
    p.state.current_pen_width = 500;
    p.thick_segment(o, o, DO_NOT_SET_LINE_WIDTH, None);
    assert_eq!(p.log, ["C0,0 500 2 0"]);

    let mut p = Rec::default();
    p.thick_segment(o, o, USE_DEFAULT_LINE_WIDTH, None);
    assert_eq!(p.log, ["W-1", "C0,0 150 2 0"]);

    // Unresolved sentinel (pen never set): wxCHECK2 returns without drawing
    let mut p = Rec::default();
    p.thick_segment(o, o, DO_NOT_SET_LINE_WIDTH, None);
    assert!(p.log.is_empty());

    let mut p = Rec::default();
    p.thick_segment(o, Vector2I::new(10, 0), 7, None);
    assert_eq!(p.log, ["W7", "U0,0", "D10,0", "Z10,0"]);
}

/// polyArc steps by 5 degrees and always ends exactly on the end angle.
#[test]
fn poly_arc_quarter() {
    let mut p = Rec::default();
    p.arc(
        Vector2D::new(0.0, 0.0),
        ANGLE_0,
        ANGLE_90,
        1000.0,
        FillT::NoFill,
        10,
    );
    // W, U start, 17 intermediate points (5..85 deg), D end, Z end
    assert_eq!(p.log.len(), 1 + 1 + 17 + 2);
    assert_eq!(p.log[1], "U1000,0");
    assert_eq!(p.log[2], "D996,87");
    assert_eq!(p.log[p.log.len() - 1], "Z0,1000");
}

#[test]
fn transform_defaults_and_arc_defs() {
    let mut p = Rec::default();
    assert_eq!(
        p.user_to_device_coordinates(Vector2I::new(3, 4)),
        Vector2D::new(3.0, -4.0)
    );
    p.state.ius_per_decimil = 2540.0;
    assert_eq!(p.plotter_arc_low_def(), 20320);
    assert_eq!(p.plotter_arc_high_def(), 5080);
}

#[test]
fn image_placeholder() {
    let mut p = Rec::default();
    p.plot_image(100, 50, Vector2I::new(1000, 1000), 2.5, 1.0);
    assert_eq!(p.log, ["R875,938 1125,1063 -1"]);
}
