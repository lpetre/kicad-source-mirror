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

//! Port of KiCad's built-in stroke font and the font-independent text layout engine
//! (`common/font/`, `common/newstroke_font.cpp`, `common/markup_parser.cpp`, the pen size helpers
//! of `common/gr_text.cpp`).
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md` and `PORT_STATUS.md`.
//!
//! Entry points:
//!
//! * [`font::get_font`] / [`font::default_font`]: font lookup (`FONT::GetFont`).
//! * [`font::Font`]: the `KIFONT::FONT` interface (`draw`, `string_boundary_limits`,
//!   `linebreak_text`, ...), implemented by [`stroke_font::StrokeFont`].
//! * [`callback_gal::TextGal`]: the drawing sink that replaces `KIGFX::GAL`;
//!   [`callback_gal::CallbackGal`] is the port of `CALLBACK_GAL`.
//! * [`markup_parser::MarkupParser`]: the `^{}`, `_{}`, `~{}` markup parser.

pub mod callback_gal;
pub mod font;
pub mod font_metrics;
pub mod glyph;
pub mod gr_text;
pub mod markup_parser;
pub mod newstroke_font;
pub mod stroke_font;

pub use callback_gal::{CallbackGal, StrokeCollector, TextGal};
pub use font::{
    Font, FontLookup, ITALIC_TILT, KICAD_FONT_NAME, TextStyleFlags, default_font, get_font,
    text_style,
};
pub use font_metrics::Metrics;
pub use glyph::{Glyph, OutlineGlyph, StrokeGlyph};
pub use markup_parser::{MarkupNode, MarkupNodeType, MarkupParser};
pub use stroke_font::StrokeFont;
