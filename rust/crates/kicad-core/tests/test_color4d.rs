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

//! Port of `qa/tests/common/test_color4d.cpp` (and the helpers of `color4d_test_utils.h`).
//! The hash checks of `Compare` use the Rust `Hash` implementation.

use std::hash::{Hash, Hasher};

use kicad_core::color4d::{Color4D, WxColour};

fn c(r: f64, g: f64, b: f64, a: f64) -> Color4D {
    Color4D::new(r, g, b, a)
}

fn is_within(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn is_color_near(col: &Color4D, other: &Color4D, tol: f64) -> bool {
    is_within(col.r, other.r, tol)
        && is_within(col.g, other.g, tol)
        && is_within(col.b, other.b, tol)
        && is_within(col.a, other.a, tol)
}

fn is_color_near_hex(col: &Color4D, r: u8, g: u8, b: u8, a: u8) -> bool {
    let tol = 0.5 / 255.0; // One bit of quantised error
    is_color_near(
        col,
        &c(
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0,
            a as f64 / 255.0,
        ),
        tol,
    )
}

/// `BOOST_CHECK_CLOSE( a, b, 0.0001 )`: relative tolerance in percent.
fn check_close(a: f64, b: f64) {
    let tol = 0.0001 / 100.0;
    let d = (a - b).abs();
    assert!(d <= tol * a.abs() && d <= tol * b.abs(), "{a} vs {b}");
}

#[test]
fn basic_ops() {
    let col = c(0.4, 0.5, 0.6, 0.7);
    assert_eq!(col.r, 0.4);
    assert_eq!(col.g, 0.5);
    assert_eq!(col.b, 0.6);
    assert_eq!(col.a, 0.7);
    let copied = col.clone();
    assert_eq!(col, copied);
    let c2 = c(0.1, 0.2, 0.3, 0.4);
    assert_ne!(col, c2);
}

#[test]
fn invert() {
    {
        let (start, expected) = (c(0.0, 0.25, 1.0, 1.0), c(1.0, 0.75, 0.0, 1.0));
        let mut col = start;
        assert_eq!(col.inverted(), expected);
        col.invert();
        assert_eq!(col, expected);
    }
}

#[test]
fn brighten() {
    for (start, factor, expected) in [
        (c(0.0, 0.0, 0.0, 1.0), 0.5, c(0.5, 0.5, 0.5, 1.0)),
        (c(0.0, 0.5, 1.0, 1.0), 0.5, c(0.5, 0.75, 1.0, 1.0)),
    ] {
        let mut col = start;
        assert_eq!(col.brightened(factor), expected);
        col.brighten(factor);
        assert_eq!(col, expected);
    }
}

#[test]
fn darken() {
    for (start, factor, expected) in [
        (c(0.0, 0.0, 0.0, 1.0), 0.5, c(0.0, 0.0, 0.0, 1.0)),
        (c(1.0, 1.0, 1.0, 1.0), 0.5, c(0.5, 0.5, 0.5, 1.0)),
    ] {
        let mut col = start;
        assert_eq!(col.darkened(factor), expected);
        col.darken(factor);
        assert_eq!(col, expected);
    }
}

#[test]
fn with_alpha() {
    for (start, factor, expected) in [
        (c(0.0, 0.0, 0.0, 1.0), 0.5, c(0.0, 0.0, 0.0, 0.5)),
        (c(0.0, 0.5, 1.0, 1.0), 0.5, c(0.0, 0.5, 1.0, 0.5)),
    ] {
        assert_eq!(start.with_alpha(factor), expected);
    }
}

#[test]
fn from_hsv() {
    let cases: [(f64, f64, f64, u8, u8, u8); 8] = [
        (10.0, 0.71, 0.66, 168, 69, 49),
        (15.0, 0.96, 0.34, 87, 24, 3),
        (120.0, 0.50, 0.50, 64, 128, 64),
        (190.0, 0.32, 0.97, 168, 234, 247),
        (240.0, 0.15, 0.75, 163, 163, 191),
        (240.0, 0.90, 0.75, 19, 19, 191),
        (310.0, 0.71, 0.66, 168, 49, 148),
        (331.0, 0.15, 0.85, 217, 184, 200),
    ];

    for (h, s, v, r, g, b) in cases {
        let mut col = Color4D::default();
        col.from_hsv(h, s, v);
        let (nh, ns, nv) = col.to_hsv(false);
        assert!(is_color_near_hex(&col, r, g, b, 0xFF));
        check_close(h, nh);
        check_close(s, ns);
        check_close(v, nv);
    }
}

#[test]
fn from_hsl() {
    let cases: [(f64, f64, f64, u8, u8, u8); 8] = [
        (10.0, 0.71, 0.66, 230, 127, 107),
        (15.0, 0.96, 0.34, 170, 45, 3),
        (120.0, 0.5, 0.5, 64, 191, 64),
        (190.0, 0.32, 0.97, 245, 249, 250),
        (240.0, 0.15, 0.75, 182, 182, 201),
        (240.0, 0.90, 0.75, 134, 134, 249),
        (310.0, 0.71, 0.66, 230, 107, 209),
        (331.0, 0.15, 0.85, 222, 211, 217),
    ];

    for (h, s, l, r, g, b) in cases {
        let mut col = Color4D::default();
        col.from_hsl(h, s, l);
        let (nh, ns, nl) = col.to_hsl();
        assert!(is_color_near_hex(&col, r, g, b, 0xFF));
        check_close(h, nh);
        check_close(s, ns);
        check_close(l, nl);
    }
}

fn wx_conv_cases() -> Vec<(WxColour, Color4D)> {
    let w = |red, green, blue, alpha| WxColour {
        red,
        green,
        blue,
        alpha,
    };
    vec![
        (w(0x00, 0x00, 0x00, 0x00), c(0.0, 0.0, 0.0, 0.0)),
        (w(0x66, 0x80, 0x99, 0xB3), c(0.4, 0.5, 0.6, 0.7)),
        (w(0xFF, 0xFF, 0xFF, 0xFF), c(1.0, 1.0, 1.0, 1.0)),
        (w(0xFF, 0x00, 0x00, 0xFF), c(0.999, 0.001, 0.0, 1.0)),
    ]
}

#[test]
fn to_wx() {
    for (wx, c4d) in wx_conv_cases() {
        assert_eq!(c4d.to_colour(), wx);
    }
}

#[test]
fn from_wx() {
    let tol = 0.5 / 255.0; // One bit of quantised error
    for (wx, c4d) in wx_conv_cases() {
        assert!(is_color_near(&Color4D::from_wx_colour(wx), &c4d, tol));
    }
}

fn hash_of(col: &Color4D) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    col.hash(&mut h);
    h.finish()
}

#[test]
fn compare() {
    let a = c(0.5, 0.5, 0.5, 0.5);
    let mut b = c(0.5, 0.5, 0.5, 0.5);

    assert_eq!(a.compare(&b), 0);
    assert_eq!(hash_of(&a), hash_of(&b));

    b.r = 0.25;
    assert!(a.compare(&b) > 0);
    assert_ne!(hash_of(&a), hash_of(&b));
    b.r = 0.75;
    assert!(a.compare(&b) < 0);
    b.r = 0.5;

    b.g = 0.25;
    assert!(a.compare(&b) > 0);
    b.g = 0.75;
    assert!(a.compare(&b) < 0);
    b.g = 0.5;

    b.b = 0.25;
    assert!(a.compare(&b) > 0);
    b.b = 0.75;
    assert!(a.compare(&b) < 0);
    b.b = 0.5;

    b.a = 0.25;
    assert!(a.compare(&b) > 0);
    b.a = 0.75;
    assert!(a.compare(&b) < 0);
}
