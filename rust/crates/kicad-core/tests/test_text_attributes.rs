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

//! Port of `qa/tests/common/test_text_attributes.cpp`.
//!
//! The C++ test also checks `std::hash<TEXT_ATTRIBUTES>` (which hashes the font pointer);
//! the port has no equivalent hash, so those checks are left out.  The default font is
//! represented by its name, "KiCad Font".

use kicad_core::color4d::{Color4D, EdaColorT};
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};

macro_rules! toggle {
    ($a:ident, $b:ident, $field:ident, $on:expr, $off:expr) => {
        $a.$field = $on;
        assert!($a > $b);
        $a.$field = $off;
        $b.$field = $on;
        assert!($a < $b);
        $b.$field = $off;
    };
}

#[test]
fn compare() {
    let mut a = TextAttributes::default();
    let mut b = TextAttributes::default();

    assert!(a == b);

    toggle!(a, b, font, Some("KiCad Font".to_string()), None);
    toggle!(a, b, halign, GrTextHAlignT::Right, GrTextHAlignT::Center);
    toggle!(a, b, valign, GrTextVAlignT::Bottom, GrTextVAlignT::Center);

    a.set_angle(90.0);
    assert!(a > b);
    a.set_angle(0.0);
    b.set_angle(90.0);
    assert!(a < b);
    b.set_angle(0.0);

    toggle!(a, b, stroke_width, 1, 0);
    toggle!(a, b, italic, true, false);
    toggle!(a, b, bold, true, false);
    toggle!(a, b, underlined, true, false);
    toggle!(
        a,
        b,
        color,
        Color4D::from(EdaColorT::RED),
        Color4D::from(EdaColorT::UNSPECIFIED_COLOR)
    );
    toggle!(a, b, mirrored, true, false);

    b.multiline = false;
    assert!(a > b);
    b.multiline = true;
    a.multiline = false;
    assert!(a < b);
    a.multiline = true;

    a.size.0 = 1;
    assert!(a > b);
    a.size.0 = 0;
    b.size.0 = 1;
    assert!(a < b);
    b.size.0 = 0;

    toggle!(a, b, keep_upright, true, false);

    assert!(a == b);
}

#[test]
fn alignment_helpers() {
    assert_eq!(GrTextHAlignT::Left as i32, -1);
    assert_eq!(GrTextVAlignT::Bottom as i32, 1);
    assert_eq!(GrTextHAlignT::Left.flipped(), GrTextHAlignT::Right);
    assert_eq!(GrTextHAlignT::Center.flipped(), GrTextHAlignT::Center);
    assert_eq!(GrTextVAlignT::Top.flipped(), GrTextVAlignT::Bottom);
    assert_eq!(GrTextHAlignT::from_sign(-5), GrTextHAlignT::Left);
    assert_eq!(GrTextHAlignT::from_sign(0), GrTextHAlignT::Center);
    let mut t = TextAttributes::default();
    t.set_angle(-90.0);
    assert_eq!(t.angle, 270.0);
    t.set_angle(-0.0);
    assert!(t.angle == 0.0 && t.angle.is_sign_positive());
    t.set_angle(720.0);
    assert_eq!(t.angle, 0.0);
}
