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

//! Port of `qa/tests/common/test_gerber_plotter.cpp` (suite `GerberPlotter`) plus unit tests
//! of the Gerber engine (work packages PLOT-03 / PLOT-04: D-code numbering, the `EndPlot`
//! splice, the arc threshold, 4.5 rounding, the pad flash table of the analysis 4.2.5 in both
//! aperture macro modes). The byte-exact comparison with the C++ engine is
//! `test_gerber_oracle.rs`.

use kicad_geom::{
    ANGLE_0, ANGLE_90, DEGREES_T, EdaAngle, ShapeLineChain, ShapePolySet, Vector2D, Vector2I,
};
use kicad_plot::gbr_metadata::{GbrApertureAttrib, GbrMetadata};
use kicad_plot::gbr_netlist_metadata::gbr_netinfo;
use kicad_plot::{FillT, GerberPlotter, Plotter, RenderSettings};

fn count_occurrences(a_haystack: &str, a_needle: &str) -> usize {
    a_haystack.matches(a_needle).count()
}

/// A plotter set up as in the C++ test: render settings, viewport `( (0,0), 2540.0, 1.0,
/// false )`, `StartPlot( "1" )`.
fn new_plotter(a_configure: impl FnOnce(&mut GerberPlotter)) -> GerberPlotter {
    let mut plotter = GerberPlotter::new();
    plotter.set_render_settings(Some(RenderSettings::default()));
    assert!(plotter.open_file("plot.gbr"));
    plotter.set_viewport(Vector2I::new(0, 0), 2540.0, 1.0, false);
    plotter.set_gerber_coordinates_format(6, false);
    a_configure(&mut plotter);
    assert!(plotter.start_plot("1"));
    plotter
}

fn finish(mut a_plotter: GerberPlotter) -> String {
    assert!(a_plotter.end_plot());
    a_plotter.output().to_string()
}

/// The body after `G04 APERTURE END LIST*`.
fn body(a_file: &str) -> &str {
    let p = a_file.find("G04 APERTURE END LIST*\n").unwrap();
    &a_file[p + "G04 APERTURE END LIST*\n".len()..]
}

// Regression test for https://gitlab.com/kicad/code/kicad/-/issues/24131
//
// A zero-length thick segment (start == end) must produce a single flash of the
// segment's aperture, not a stroked outline of a phantom 0-width aperture.
// The pre-fix code delegated to PLOTTER::ThickSegment with width set to the
// DO_NOT_SET_LINE_WIDTH sentinel (-2). PLOTTER::ThickSegment then called
// Circle( start, -2, FILLED, 0 ), which the GERBER_PLOTTER turned into a tiny
// polyArc with line width 0, producing an ADD11C,0 aperture and ~76 D01
// commands sampling a degenerate radius.
#[test]
fn zero_length_segment_emits_single_flash() {
    let mut plotter = new_plotter(|_| {});

    let width = 2 * 1_000_000; // 2 mm in nm
    let origin = Vector2I::new(0, 0);

    plotter.thick_segment(origin, origin, width, None);
    let buffer = finish(plotter);

    // The 2 mm aperture must be present.
    assert!(
        buffer.contains("C,2.000000"),
        "Expected 2.000000 mm circle aperture in gerber output"
    );

    // No phantom 0 mm aperture should leak out.
    assert!(
        !buffer.contains("C,0.000000"),
        "Spurious 0.000000 mm aperture present (issue 24131 regression)"
    );

    // No circular interpolation commands either; the degenerate point should be
    // a flash, not an arc.
    assert!(
        !buffer.contains("G02") && !buffer.contains("G03"),
        "Unexpected circular interpolation in zero-length segment plot"
    );

    // The pre-fix output had about 76 D01 commands. The expected v9.0.9 output is
    // exactly one D02 (move) immediately followed by one D01 (flash) at the origin.
    assert_eq!(count_occurrences(&buffer, "D01*"), 1);
    assert!(
        buffer.contains("X0Y0D02*") && buffer.contains("X0Y0D01*"),
        "Expected single zero-length segment flash at origin"
    );
}

#[test]
fn header_and_splice() {
    let mut plotter = GerberPlotter::new();
    plotter.set_render_settings(Some(RenderSettings::default()));
    plotter.set_creator("PCBNEW");
    plotter.set_build_version("10.99.0");
    plotter.set_creation_date("2026-01-02 03:04:05");
    plotter.add_line_to_header("%TF.FileFunction,Copper,L1,Top*%");
    plotter.add_line_to_header("");
    plotter.open_file("x.gbr");
    plotter.set_viewport(Vector2I::new(0, 0), 2540.0, 1.0, false);
    plotter.set_gerber_coordinates_format(5, false);
    plotter.start_plot("1");
    plotter.flash_pad_circle(Vector2I::new(1_000_000, 2_000_000), 500_000, None);
    let file = finish(plotter);

    assert_eq!(
        file,
        "%TF.FileFunction,Copper,L1,Top*%\n\
         %FSLAX45Y45*%\n\
         G04 Gerber Fmt 4.5, Leading zero omitted, Abs format (unit mm)*\n\
         G04 Created by KiCad (PCBNEW 10.99.0) date 2026-01-02 03:04:05*\n\
         %MOMM*%\n\
         %LPD*%\n\
         G01*\n\
         G04 APERTURE LIST*\n\
         %ADD10C,0.500000*%\n\
         G04 APERTURE END LIST*\n\
         D10*\n\
         X100000Y-200000D03*\n\
         M02*\n"
    );
}

#[test]
fn dcode_numbering_and_reselection() {
    let mut plotter = new_plotter(|_| {});
    let mut md = GbrMetadata::new();
    md.set_aperture_attrib(GbrApertureAttrib::Conductor);

    plotter.thick_segment(Vector2I::new(0, 0), Vector2I::new(10, 0), 200_000, None); // D10
    plotter.flash_pad_circle(Vector2I::new(0, 0), 200_000, None); // D11: AT_CIRCLE != AT_PLOTTING
    plotter.thick_segment(
        Vector2I::new(0, 0),
        Vector2I::new(10, 0),
        200_000,
        Some(&md),
    ); // D12
    plotter.thick_segment(Vector2I::new(0, 0), Vector2I::new(10, 0), 200_000, None); // D10 again
    let file = finish(plotter);

    assert!(file.contains(
        "%ADD10C,0.200000*%\n%ADD11C,0.200000*%\n%TA.AperFunction,Conductor*%\n%ADD12C,0.200000*%\n%TD*%\n"
    ));
    let b = body(&file);
    assert_eq!(
        b.lines().filter(|l| l.starts_with('D')).collect::<Vec<_>>(),
        ["D10*", "D11*", "D12*", "D10*"]
    );
}

#[test]
fn arc_threshold() {
    // arc length < 100 IU: polyline (polyArc), otherwise G75/G02 arc.
    let mut plotter = new_plotter(|_| {});
    plotter.arc(
        Vector2D::new(0.0, 0.0),
        ANGLE_0,
        ANGLE_90,
        60.0,
        FillT::NoFill,
        1000,
    );
    let short = finish(plotter);
    assert!(!short.contains("G75*"));

    let mut plotter = new_plotter(|_| {});
    plotter.arc(
        Vector2D::new(0.0, 0.0),
        ANGLE_0,
        ANGLE_90,
        1_000.0,
        FillT::NoFill,
        1000,
    );
    let long = finish(plotter);
    assert!(body(&long).contains("X1000Y0D02*\nG75*\nG02*\nX0Y-1000I-1000J0D01*\nG01*\n"));

    // |angle| < 0.1 degree: polyline even for a large radius
    let mut plotter = new_plotter(|_| {});
    plotter.arc(
        Vector2D::new(0.0, 0.0),
        ANGLE_0,
        EdaAngle::new(0.05, DEGREES_T),
        1.0e7,
        FillT::NoFill,
        1000,
    );
    assert!(!finish(plotter).contains("G75*"));
}

#[test]
fn format_45_rounding() {
    // 4.5 format: the device factor is 0.09999999999999999, so the rounding of coordinates
    // ending in 5 nm depends on the magnitude (analysis 4.1).
    let mut plotter = new_plotter(|p| p.set_gerber_coordinates_format(5, false));
    plotter.move_to(Vector2I::new(15, -15));
    plotter.move_to(Vector2I::new(100_005, 0));
    plotter.move_to(Vector2I::new(0, 0));
    let file = finish(plotter);
    let b = body(&file);
    assert!(b.contains("X1Y1D02*\n"), "{b}");
    assert!(b.contains("X10001Y0D02*\n"), "{b}");
    assert!(b.contains("X0Y0D02*\n"), "{b}");
}

#[test]
fn net_attributes_state_machine() {
    let mut plotter = new_plotter(|_| {});
    let mut pad = GbrMetadata::new();
    pad.set_net_attrib_type(gbr_netinfo::ALL);
    pad.set_pad_name("1", false, false);
    pad.set_cmp_reference("R1");
    pad.set_net_name("GND");
    let mut net_only = GbrMetadata::new();
    net_only.set_net_attrib_type(gbr_netinfo::NET);
    net_only.set_net_name("GND");

    plotter.flash_pad_circle(Vector2I::new(0, 0), 1000, Some(&pad));
    plotter.flash_pad_circle(Vector2I::new(0, 0), 1000, Some(&pad)); // nothing new
    plotter.flash_pad_circle(Vector2I::new(0, 0), 1000, Some(&net_only)); // TD + full
    // nothing: the TD of the previous object emptied the dictionary although the file's
    // dictionary is not empty (analysis 4.2.3)
    plotter.end_block(None);
    let file = finish(plotter);
    assert_eq!(
        body(&file),
        "D10*\n%TO.P,R1,1*%\n%TO.N,GND*%\nX0Y0D03*\nX0Y0D03*\n%TD*%\n%TO.N,GND*%\nX0Y0D03*\nM02*\n"
    );
}

/// The flash table of the analysis (4.2.5), in both aperture macro modes.
#[test]
fn pad_flash_table() {
    let pos = Vector2I::new(1_000_000, 1_000_000);
    let size = Vector2I::new(2_000_000, 1_000_000);
    let a30 = EdaAngle::new(30.0, DEGREES_T);

    for disable in [false, true] {
        let mut plotter = new_plotter(|p| p.disable_apert_macros(disable));
        plotter.flash_pad_circle(pos, 1_000_000, None);
        plotter.flash_pad_oval(pos, size, EdaAngle::new(90.0, DEGREES_T), None);
        plotter.flash_pad_oval(pos, size, EdaAngle::new(-270.0, DEGREES_T), None);
        plotter.flash_pad_rect(pos, size, EdaAngle::new(270.0, DEGREES_T), None);
        let file = finish(plotter);
        assert!(file.contains(
            "%ADD10C,1.000000*%\n%ADD11O,1.000000X2.000000*%\n%ADD12R,1.000000X2.000000*%\n"
        ));
        assert!(!file.contains("Aperture macros list"));

        let mut plotter = new_plotter(|p| p.disable_apert_macros(disable));
        plotter.flash_pad_oval(pos, size, a30, None);
        plotter.flash_pad_rect(pos, size, a30, None);
        plotter.flash_pad_round_rect(pos, size, 200_000, ANGLE_0, None);
        let corners = [
            Vector2I::new(-500_000, 400_000),
            Vector2I::new(-400_000, -400_000),
            Vector2I::new(400_000, -400_000),
            Vector2I::new(500_000, 400_000),
        ];
        plotter.flash_pad_trapez(pos, &corners, a30, None);
        let mut custom = ShapePolySet::new();
        custom.add_outline(&ShapeLineChain::from_points(
            &[
                pos,
                pos + Vector2I::new(500_000, 0),
                pos + Vector2I::new(0, 500_000),
            ],
            true,
        ));
        plotter.flash_pad_custom(pos, size, a30, &custom, None);
        plotter.flash_regular_polygon(pos, 1_000_000, 6, a30, None);
        let file = finish(plotter);

        if !disable {
            for macro_name in [
                "%AMHorizOval*",
                "%AMRotRect*",
                "%AMRoundRect*",
                "%AMOutline4P*",
                "%AMFreePoly0*",
            ] {
                assert!(file.contains(macro_name), "{macro_name} missing:\n{file}");
            }
            assert!(
                file.contains(
                    "%ADD10HorizOval,1.000000X0.433013X0.250000X-0.433013X-0.250000X0*%\n"
                )
            );
            assert!(file.contains("%ADD11RotRect,2.000000X1.000000X30.000000*%\n"));
            assert!(file.contains(
                "%ADD12RoundRect,0.200000X-0.800000X-0.300000X0.800000X-0.300000X0.800000X0.300000X-0.800000X0.300000X0*%\n"
            ));
            assert!(file.contains(
                "%ADD13Outline4P,-0.500000X-0.400000X-0.400000X0.400000X0.400000X0.400000X0.500000X-0.400000X30.000000*%\n"
            ));
            assert!(file.contains("%ADD14FreePoly0,30.000000*%\n"));
            assert!(file.contains("%ADD15P,1.000000X6X30.000000*%\n"));
            assert_eq!(count_occurrences(body(&file), "G36*"), 0);
        } else {
            // every non-primitive shape becomes a region; only the regular polygon remains
            assert!(!file.contains("Aperture macros list"));
            assert!(file.contains("%ADD10P,1.000000X6X30.000000*%\n"), "{file}");
            assert_eq!(count_occurrences(body(&file), "G36*"), 5);
            // round rect (and rotated oval) regions are made of arcs
            assert!(body(&file).contains("G75*"));
        }
    }
}

#[test]
fn chamfered_pad_outline_macros() {
    // FlashPadChamferRoundRect without rounded corner: the polygon is built at the origin
    // with rotation 0 and its corner count selects Outline4P..8P.
    let corners5 = [
        Vector2I::new(-500, -300),
        Vector2I::new(400, -300),
        Vector2I::new(500, -200),
        Vector2I::new(500, 300),
        Vector2I::new(-500, 300),
    ];
    let transform =
        |pos: Vector2I, _s: Vector2I, rot: EdaAngle, r: i32, _ratio: f64, _p: i32, err: i32| {
            assert_eq!((pos, rot, r, err), (Vector2I::new(0, 0), ANGLE_0, 0, 5080));
            let mut ps = ShapePolySet::new();
            ps.add_outline(&ShapeLineChain::from_points(&corners5, true));
            ps
        };
    let mut plotter = new_plotter(|_| {});
    plotter.flash_pad_chamfer_round_rect(
        Vector2I::new(10, 20),
        Vector2I::new(1000, 600),
        0,
        0.1,
        2,
        ANGLE_90,
        None,
        &transform,
    );
    let file = finish(plotter);
    assert!(file.contains("%AMOutline5P*") && file.contains("%AMOutline8P*"));
    assert!(file.contains(
        "%ADD10Outline5P,-0.000500X0.000300X0.000400X0.000300X0.000500X0.000200X0.000500X-0.000300X-0.000500X-0.000300X90.000000*%\n"
    ));
    assert!(body(&file).starts_with("D10*\nX10Y-20D03*\n"));
}

#[test]
fn x1_comments_and_polarity() {
    let mut plotter = new_plotter(|p| p.use_x2format(false));
    let mut md = GbrMetadata::new();
    md.set_aperture_attrib(GbrApertureAttrib::Conductor);
    md.set_net_attrib_type(gbr_netinfo::NET);
    md.set_net_name("N1");
    plotter.set_layer_polarity(false);
    plotter.plot_gerber_region(
        &[
            Vector2I::new(0, 0),
            Vector2I::new(1000, 0),
            Vector2I::new(0, 1000),
        ],
        Some(&md),
    );
    plotter.set_layer_polarity(true);
    plotter.clear_all_attributes();
    let file = finish(plotter);
    assert_eq!(
        body(&file),
        "%LPC*%\nG04 #@! TA.AperFunction,Conductor*\nG04 #@! TO.N,N1*\nG36*\nX0Y0D02*\nG01*\n\
         X1000Y0D01*\nX0Y-1000D01*\nX0Y0D01*\nG37*\nG04 #@! TD.AperFunction*\n%LPD*%\nG04 #@! TD*\nM02*\n"
    );
}
