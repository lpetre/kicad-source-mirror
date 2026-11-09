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

//! Port of `qa/tests/common/test_title_block.cpp`.

use kicad_core::title_block::TitleBlock;

fn fixture() -> TitleBlock {
    let mut tb = TitleBlock::new();
    tb.set_title("title");
    tb.set_date("date");
    tb.set_company("company");
    // leave revision blank
    // set more than one comment to make sure the indexing of comments works
    tb.set_comment(0, "comment1");
    tb.set_comment(1, "comment2");
    tb.set_comment(2, "comment3");
    tb.set_comment(3, "comment4");
    tb
}

#[test]
fn simple_access() {
    let tb = fixture();
    assert_eq!("title", tb.title());
    assert_eq!("date", tb.date());
    assert_eq!("company", tb.company());
    // This one is blank
    assert_eq!("", tb.revision());
    assert_eq!("comment1", tb.comment(0));
    assert_eq!("comment2", tb.comment(1));
    assert_eq!("comment3", tb.comment(2));
    assert_eq!("comment4", tb.comment(3));
}

#[test]
fn copy() {
    let tb = fixture();
    let tb_cpy = tb.clone();
    assert_eq!("title", tb_cpy.title());
    assert_eq!("comment1", tb_cpy.comment(0));
    assert_eq!("comment2", tb_cpy.comment(1));
}
