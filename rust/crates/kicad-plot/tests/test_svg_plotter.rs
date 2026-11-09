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

//! Port of `qa/tests/common/test_svg_plotter.cpp` (suite `SvgPlotter`), plus unit tests of
//! the SVG engine and the PSLIKE pad flashes.

use kicad_core::color4d::Color4D;
use kicad_core::page_info::PageInfo;
use kicad_core::stroke_params::LineStyle;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::Metrics;
use kicad_geom::{ANGLE_0, Box2I, EdaAngle, Vector2D, Vector2I};
use kicad_plot::plotter_svg::xml_esc;
use kicad_plot::{FillT, Plotter, RenderSettings, SvgPlotter};

/// Plot one filled shape at the requested colour and return the whole SVG document.
fn plot_filled_shape(a_color: Color4D, a_mirrored: bool, a_fit_to_board: bool) -> String {
    let mut plotter = SvgPlotter::new();
    let mut page_info = PageInfo::default();

    page_info.set_width_mils(1000.0);
    page_info.set_height_mils(1000.0);
    plotter.set_page_settings(&page_info);
    plotter.set_viewport(Vector2I::new(0, 0), 1.0, 1.0, a_mirrored);

    // If fit to board, define the board at (20000, 20000) with size 10000x10000 IU
    if a_fit_to_board {
        plotter.set_plot_bbox(&Box2I::new(
            Vector2I::new(20000, 20000),
            Vector2I::new(10000, 10000),
        ));
    }

    plotter.set_color_mode(true);

    assert!(plotter.open_file("/tmp/kicad_svg_plotter/plot.svg"));
    assert!(plotter.start_plot("1"));

    plotter.set_color(&a_color);

    let shape_size = Vector2I::new(4000, 4000);

    // Draw the shape in the page somewhere.
    let shape_origin = if a_fit_to_board {
        Vector2I::new(20000, 20000)
    } else {
        Vector2I::new(100, 100)
    };

    // Actually draw the shape.
    plotter.rect(
        shape_origin,
        shape_origin + shape_size,
        FillT::FilledShape,
        10,
        0,
    );

    assert!(plotter.end_plot());
    plotter.output().to_string()
}

/// Every distinct value following `key` (digits and '.').
fn values_after(a_svg: &str, key: &str) -> std::collections::BTreeSet<String> {
    let mut values = std::collections::BTreeSet::new();
    let mut from = 0;

    while let Some(pos) = a_svg[from..].find(key) {
        let start = from + pos + key.len();
        let end = a_svg[start..]
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .map(|e| start + e)
            .unwrap_or(a_svg.len());
        values.insert(a_svg[start..end].to_string());
        from = from + pos + 1;
    }

    values
}

fn fill_opacities(a_svg: &str) -> std::collections::BTreeSet<String> {
    values_after(a_svg, "fill-opacity:")
}

fn stroke_opacities(a_svg: &str) -> std::collections::BTreeSet<String> {
    values_after(a_svg, "stroke-opacity:")
}

/// `KI_TEST::ParseViewBox` of the root element.
fn view_box(a_svg: &str) -> [f64; 4] {
    let start = a_svg.find("viewBox=\"").expect("viewBox") + 9;
    let end = start + a_svg[start..].find('"').unwrap();
    let v: Vec<f64> = a_svg[start..end]
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    [v[0], v[1], v[2], v[3]]
}

/// The attribute of the first `<rect>` (`KI_TEST::FindFirstRect`).
fn rect_attribute(a_svg: &str, a_name: &str) -> String {
    let rect = &a_svg[a_svg.find("<rect ").expect("a rect")..];
    let key = format!(" {a_name}=\"");
    let start = rect.find(&key).unwrap() + key.len();
    let end = start + rect[start..].find('"').unwrap();
    rect[start..end].to_string()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.00001 * a.abs().max(b.abs()).max(1e-300) || a == b
}

#[test]
fn fill_opacity_matches_color_alpha() {
    let values = fill_opacities(&plot_filled_shape(
        Color4D::new(1.0, 0.0, 0.0, 0.5),
        false,
        false,
    ));

    // The plotter writes m_precision (4) digits, so a half-transparent brush is exactly 0.5000
    assert!(values.contains("0.5000"));
    assert!(!values.contains("0.0000"));
}

#[test]
fn fill_opacity_fully_opaque() {
    let values = fill_opacities(&plot_filled_shape(
        Color4D::new(0.0, 0.0, 1.0, 1.0),
        false,
        false,
    ));

    assert!(values.contains("1.0000"));

    // Nothing may be drawn semi-transparently when the brush is opaque
    for value in &values {
        assert_eq!(value, "1.0000");
    }
}

#[test]
fn fill_opacity_preserved_through_set_color() {
    let cases = [
        (0.0, "0.0000"),
        (0.25, "0.2500"),
        (0.5, "0.5000"),
        (0.75, "0.7500"),
        (1.0, "1.0000"),
    ];

    for (alpha, expected) in cases {
        let values = fill_opacities(&plot_filled_shape(
            Color4D::new(0.5, 0.5, 0.5, alpha),
            false,
            false,
        ));

        assert!(values.contains(expected), "alpha {alpha}");

        // No other requested alpha may appear, or SetColor is not reaching the output
        for (_, other) in cases {
            if other != expected && other != "1.0000" {
                assert!(!values.contains(other), "alpha {alpha}: {other}");
            }
        }
    }
}

#[test]
fn stroke_opacity_matches_color_alpha() {
    let svg = plot_filled_shape(Color4D::new(1.0, 0.0, 0.0, 0.25), false, false);
    let values = stroke_opacities(&svg);

    assert!(values.contains("0.2500"));
}

#[test]
fn default_plot_view_box_is_origin() {
    let svg = plot_filled_shape(Color4D::new(1.0, 0.0, 0.0, 0.25), false, false);
    let vb = view_box(&svg);

    // The default plotter origin is at (0, 0) IU
    // And the page is 1000 mils square, so the viewBox is 25.4 * 25.4 mm
    assert!(close(vb[0], 0.0) && close(vb[1], 0.0));
    assert!(close(vb[2], 25.4) && close(vb[3], 25.4));

    // The rectangle's left edge is 10 mils = 0.254 mm
    assert_eq!(rect_attribute(&svg, "x"), "0.254000");
}

#[test]
fn mirrored_svg_preserves_plot_origin() {
    let svg = plot_filled_shape(Color4D::new(1.0, 0.0, 0.0, 0.25), true, false);
    let vb = view_box(&svg);

    // We didn't fit to the board, so the unmirrored page was from 0 to 25.4 mm (in x)
    // And the mirrored page is from -25.4 to 0 mm (in x)
    assert!(close(vb[0], -25.4) && close(vb[1], 0.0));
    assert!(close(vb[2], 25.4) && close(vb[3], 25.4));

    // And the rectangle's left edge is -410 mils = -10.414 mm
    assert_eq!(rect_attribute(&svg, "x"), "-10.414000");
}

#[test]
fn fit_to_board_uses_view_box() {
    let svg = plot_filled_shape(Color4D::new(1.0, 0.0, 0.0, 0.25), false, true);
    let vb = view_box(&svg);

    // The board is 1000 mils square
    // And the viewBox is offset by 2000 mils
    assert!(close(vb[0], 50.8) && close(vb[1], 50.8));
    assert!(close(vb[2], 25.4) && close(vb[3], 25.4));

    // The rectangle's left edge is +2000mils
    assert_eq!(rect_attribute(&svg, "x"), "50.800000");
}

#[test]
fn mirrored_fit_to_board_uses_mirrored_view_box() {
    let svg = plot_filled_shape(Color4D::new(1.0, 0.0, 0.0, 0.25), true, true);
    let vb = view_box(&svg);

    // For the mirrored case, the viewBox is flipped horizontally
    assert!(close(vb[0], -76.2) && close(vb[1], 50.8));
    assert!(close(vb[2], 25.4) && close(vb[3], 25.4));

    // The rectangle spans 20000 to 24000 IU, which mirrors to -20000 to -24000 IU.
    assert_eq!(rect_attribute(&svg, "x"), "-60.960000");
}

// ---- additional unit tests (not in the C++ suite) ----

fn a4_plotter() -> SvgPlotter {
    let mut p = SvgPlotter::new();
    p.set_render_settings(Some(RenderSettings::default()));
    p.set_page_settings(&PageInfo::default());
    p.set_viewport(Vector2I::new(0, 0), 2540.0, 1.0, false);
    p.set_creator("PCBNEW");
    p.set_creation_date("2026-01-02T03:04:05");
    p.open_file("/tmp/out/board-F_Cu.svg");
    p
}

#[test]
fn header_and_footer_literal() {
    let mut p = a4_plotter();
    p.start_plot("1");
    p.end_plot();
    let expected = "<?xml version=\"1.0\" standalone=\"no\"?>\n \
<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \n \
\"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\"> \n<svg\n  \
xmlns:svg=\"http://www.w3.org/2000/svg\"\n  xmlns=\"http://www.w3.org/2000/svg\"\n  \
xmlns:xlink=\"http://www.w3.org/1999/xlink\"\n  \
xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"\n  version=\"1.1\"\n  \
width=\"419.9890mm\" height=\"297.0022mm\" viewBox=\"0.0000 0.0000 419.9890 297.0022\">\n\
<title>SVG Image created as board-F_Cu.svg date 2026-01-02T03:04:05 </title>\n  \
<desc>Image generated by PCBNEW </desc>\n\
<g style=\"fill:#000000; fill-opacity:1.0000;stroke:#000000; stroke-opacity:1.0000;\n\
stroke-linecap:round; stroke-linejoin:round;\"\n transform=\"translate(0 0) scale(1 1)\">\n\
</g> \n</svg>\n";
    assert_eq!(p.output(), expected);
}

#[test]
fn style_group_and_pen_path() {
    let mut p = a4_plotter();
    p.start_plot("1");
    let start = p.output().len();
    p.set_current_line_width(100000, None);
    p.move_to(Vector2I::new(1000000, 2000000));
    p.line_to(Vector2I::new(3000000, 2000000));
    p.line_to(Vector2I::new(3000000, 2000000)); // same point, dropped
    p.pen_finish();
    let body = p.output()[start..].to_string();
    assert_eq!(
        body,
        "</g>\n<g style=\"fill:none; \nstroke:#000000; stroke-width:0.1000; \
stroke-opacity:1.0000; \nstroke-linecap:round; stroke-linejoin:round;\
stroke-dasharray:none;\">\n<path d=\"M1.0000 2.0000\nL3.0000 2.0000\n\" />\n"
    );
}

#[test]
fn dash_styles() {
    let mut p = a4_plotter();
    p.start_plot("1");
    p.set_current_line_width(100000, None);
    p.set_dash(100000, LineStyle::Dash);
    p.thick_segment(Vector2I::new(0, 0), Vector2I::new(1000000, 0), 100000, None);
    assert!(p.output().contains("stroke-dasharray:1.1000,0.4000;"));
    p.set_dash(100000, LineStyle::Dot);
    p.thick_segment(Vector2I::new(0, 0), Vector2I::new(1000000, 0), 100000, None);
    assert!(p.output().contains("stroke-dasharray:0.020000,0.400000;"));
    p.set_dash(100000, LineStyle::DashDotDot);
    p.thick_segment(Vector2I::new(0, 0), Vector2I::new(1000000, 0), 100000, None);
    assert!(
        p.output()
            .contains("stroke-dasharray:1.100000,0.400000,0.020000,0.400000,0.020000,0.400000;")
    );
}

#[test]
fn colour_modes() {
    // black and white: everything but WHITE is black; negative inverts
    let mut p = a4_plotter();
    p.start_plot("1");
    p.set_color(&Color4D::new(1.0, 0.0, 0.0, 0.5));
    p.circle(Vector2I::new(0, 0), 1000, FillT::FilledShape, 0);
    assert!(p.output().contains("fill:#000000; fill-opacity:1.0000;"));
    p.set_negative(true);
    p.set_color(&Color4D::new(1.0, 0.0, 0.0, 0.5));
    p.circle(Vector2I::new(0, 0), 1000, FillT::FilledShape, 0);
    assert!(p.output().contains("fill:#FFFFFF; fill-opacity:1.0000;"));
    p.set_color(&Color4D::WHITE);
    p.circle(Vector2I::new(0, 0), 1000, FillT::FilledShape, 0);
    assert!(
        p.output()
            .ends_with("<circle cx=\"0.0000\" cy=\"0.0000\" r=\"0.0005\" /> \n")
    );
    p.set_color_mode(true);
    p.set_color(&Color4D::new(0.2, 0.4, 0.6, 0.5));
    p.circle(Vector2I::new(0, 0), 1000, FillT::FilledShape, 0);
    // negative colour mode: 255 * 0.8 = 204, 255 * 0.6 = 153 (truncated), 255 * 0.4 = 102
    assert!(p.output().contains("fill:#CC9966; fill-opacity:0.5000;"));
}

#[test]
fn rect_degenerate_is_a_line_and_regular_polygon_does_nothing() {
    let mut p = a4_plotter();
    p.start_plot("1");
    p.rect(
        Vector2I::new(0, 0),
        Vector2I::new(1000000, 0),
        FillT::NoFill,
        0,
        0,
    );
    assert!(
        p.output()
            .ends_with("<line x1=\"0.0000\" y1=\"0.0000\" x2=\"1.0000\" y2=\"0.0000\" />\n")
    );
    let len = p.output().len();
    p.flash_regular_polygon(Vector2I::new(0, 0), 1000, 6, ANGLE_0, None);
    assert_eq!(p.output().len(), len);
}

#[test]
fn arc_and_bezier() {
    let mut p = a4_plotter();
    p.start_plot("1");
    p.arc(
        Vector2D::new(0.0, 0.0),
        EdaAngle::from_degrees(0.0),
        EdaAngle::from_degrees(90.0),
        1000000.0,
        FillT::NoFill,
        100000,
    );
    let out = p.output().to_string();
    assert!(
        out.ends_with("<path d=\"M0.0000 1.0000 A1.0000 1.0000 0.0 0 0 1.0000 0.0000\" />\n"),
        "{out}"
    );
    p.bezier_curve(
        Vector2I::new(0, 0),
        Vector2I::new(1000000, 0),
        Vector2I::new(1000000, 1000000),
        Vector2I::new(0, 1000000),
        0,
        100000,
    );
    assert!(
        p.output().ends_with(
            "<path d=\"M0.0000,0.0000 C1.0000,0.0000 1.0000,1.0000 0.0000,1.0000\" />\n"
        )
    );
}

#[test]
fn text_hidden_element_and_stroked_glyphs() {
    let mut p = a4_plotter();
    p.start_plot("1");
    let mut attrs = TextAttributes::default();
    attrs.size = (1000000, 1000000);
    attrs.stroke_width = 150000;
    attrs.halign = GrTextHAlignT::Left;
    attrs.valign = GrTextVAlignT::Bottom;
    p.plot_text(
        Vector2I::new(10000000, 10000000),
        &Color4D::BLACK,
        "A<&>",
        &attrs,
        None,
        &Metrics::default(),
        None,
    );
    let out = p.output();
    assert!(out.contains("<text x=\"10.0000\" y=\"10.0000\"\n"));
    assert!(out.contains(
        "text-anchor=\"start\" opacity=\"0\" stroke-opacity=\"0\">A&lt;&amp;&gt;</text>\n"
    ));
    assert!(out.contains("<g class=\"stroked-text\"><desc>A&lt;&amp;&gt;</desc>\n"));
    assert!(out.contains("<path d=\"M"));
    assert!(out.ends_with("\" />\n</g>"));
    assert!(p.state().outline_font_fallbacks.is_empty());
}

#[test]
fn outline_default_font_falls_back_to_stroke_font() {
    let mut p = a4_plotter();
    let mut rs = RenderSettings::default();
    rs.set_default_font("Arial");
    p.set_render_settings(Some(rs));
    p.start_plot("1");
    p.text(
        Vector2I::new(0, 0),
        &Color4D::BLACK,
        "X",
        ANGLE_0,
        Vector2I::new(1000000, 1000000),
        GrTextHAlignT::Center,
        GrTextVAlignT::Center,
        100000,
        false,
        false,
        true,
        None,
        &Metrics::default(),
        None,
    );
    assert_eq!(p.state().outline_font_fallbacks, vec!["Arial".to_string()]);
    assert!(p.output().contains("<path d=\"M"));
}

#[test]
fn xml_escaping() {
    assert_eq!(
        xml_esc("a<b>&\r\"\t\n", false),
        "a&lt;b&gt;&amp;&#xD;\"\t\n"
    );
    assert_eq!(
        xml_esc("a<b>&\r\"\t\n", true),
        "a&lt;b&gt;&amp;&#xD;&quot;&#x9;&#xA;"
    );
}
