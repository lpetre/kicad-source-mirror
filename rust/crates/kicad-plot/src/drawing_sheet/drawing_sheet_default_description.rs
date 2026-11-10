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

//! Port of `common/drawing_sheet/drawing_sheet_default_description.cpp`: the built-in
//! default drawing sheet and the "empty" drawing sheet, as S-expression texts.
//!
//! Items use coordinates. A coordinate is defined relative to a page corner: the X pos, the
//! Y pos, and the corner which is the coordinate origin (the default is the bottom right
//! corner), e.g. `(start 10 0 ltcorner)` or `(start 20 10)`. The direction depends on the
//! corner: a positive coordinate defines a point from the corner origin, to the opposite
//! corner. Items are defined by a name, coordinates in mm and some attributes, and can be
//! repeated: `(repeat 2) (incrx 2) (incry 2)` repeats the item 2 times, and coordinates are
//! incremented by 2 on X direction, and 2 on Y direction.
//!
//! Sizes of the default sheet: height of the coordinates band 2.0 mm, coordinates text size
//! 1.3 mm, default text size 1.5 mm, default line width 0.15 mm, frame ref pitch 50 mm.

/// `defaultDrawingSheet`: the default drawing sheet (sizes are in mm).
pub const DEFAULT_DRAWING_SHEET: &str = concat!(
    "(kicad_wks (version 20210606) (generator pl_editor)\n",
    "(setup (textsize 1.5 1.5)(linewidth 0.15)(textlinewidth 0.15)\n",
    "(left_margin 10)(right_margin 10)(top_margin 10)(bottom_margin 10))\n",
    "(rect (name \"\") (start 110 34) (end 2 2) (comment \"rect around the title block\"))\n",
    "(rect (name \"\") (start 0 0 ltcorner) (end 0 0) (repeat 2) (incrx 2) (incry 2))\n",
    "(line (name \"\") (start 50 2 ltcorner) (end 50 0 ltcorner) (repeat 30) (incrx 50))\n",
    "(tbtext \"1\" (name \"\") (pos 25 1 ltcorner) (font (size 1.3 1.3)) (repeat 100) (incrx 50))\n",
    "(line (name \"\") (start 50 2 lbcorner) (end 50 0 lbcorner) (repeat 30) (incrx 50))\n",
    "(tbtext \"1\" (name \"\") (pos 25 1 lbcorner) (font (size 1.3 1.3)) (repeat 100) (incrx 50))\n",
    "(line (name \"\") (start 0 50 ltcorner) (end 2 50 ltcorner) (repeat 30) (incry 50))\n",
    "(tbtext \"A\" (name \"\") (pos 1 25 ltcorner) (font (size 1.3 1.3)) (justify center) (repeat 100) (incry 50))\n",
    "(line (name \"\") (start 0 50 rtcorner) (end 2 50 rtcorner) (repeat 30) (incry 50))\n",
    "(tbtext \"A\" (name \"\") (pos 1 25 rtcorner) (font (size 1.3 1.3)) (justify center) (repeat 100) (incry 50))\n",
    "(tbtext \"Date: ${ISSUE_DATE}\" (name \"\") (pos 87 6.9))\n",
    "(line (name \"\") (start 110 5.5) (end 2 5.5))\n",
    "(tbtext \"${KICAD_VERSION}\" (name \"\") (pos 109 4.1) (comment \"Kicad version\"))\n",
    "(line (name \"\") (start 110 8.5) (end 2 8.5))\n",
    "(tbtext \"Rev: ${REVISION}\" (name \"\") (pos 24 6.9) (font bold))\n",
    "(tbtext \"Size: ${PAPER}\" (name \"\") (pos 109 6.9) (comment \"Paper format name\"))\n",
    "(tbtext \"Id: ${#}/${##}\" (name \"\") (pos 24 4.1) (comment \"Sheet id\"))\n",
    "(line (name \"\") (start 110 12.5) (end 2 12.5))\n",
    "(tbtext \"Title: ${TITLE}\" (name \"\") (pos 109 10.7) (font (size 2 2) bold italic))\n",
    "(tbtext \"File: ${FILENAME}\" (name \"\") (pos 109 14.3))\n",
    "(line (name \"\") (start 110 18.5) (end 2 18.5))\n",
    "(tbtext \"Sheet: ${SHEETPATH}\" (name \"\") (pos 109 17))\n",
    "(tbtext \"${COMPANY}\" (name \"\") (pos 109 20) (font bold) (comment \"Company name\"))\n",
    "(tbtext \"${COMMENT1}\" (name \"\") (pos 109 23) (comment \"Comment 0\"))\n",
    "(tbtext \"${COMMENT2}\" (name \"\") (pos 109 26) (comment \"Comment 1\"))\n",
    "(tbtext \"${COMMENT3}\" (name \"\") (pos 109 29) (comment \"Comment 2\"))\n",
    "(tbtext \"${COMMENT4}\" (name \"\") (pos 109 32) (comment \"Comment 3\"))\n",
    "(line (name \"\") (start 90 8.5) (end 90 5.5))\n",
    "(line (name \"\") (start 26 8.5) (end 26 2))\n",
    ")\n",
);

/// `emptyDrawingSheet`: an "empty" drawing sheet; there is a 0 length line to fool
/// something somewhere.
pub const EMPTY_DRAWING_SHEET: &str = concat!(
    "(kicad_wks (version 20210606) (generator pl_editor)\n",
    "(setup (textsize 1.5 1.5)(linewidth 0.15)(textlinewidth 0.15)\n",
    "(left_margin 10)(right_margin 10)(top_margin 10)(bottom_margin 10))\n",
    "(line (name \"segm1:Line\") (start 0 0) (end 0 0))\n",
    ")\n",
);
