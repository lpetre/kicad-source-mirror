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

//! Port of `pcbnew/convert_shape_list_to_polygon.h` / `.cpp`. Package M21 ports the
//! functions; the error handler type is here already because the board outline
//! (`board/outline.rs`) and the footprint courtyards (`footprint/geometry.rs`) take it.

use kicad_geom::Vector2I;

use crate::ids::ItemId;

/// `OUTLINE_ERROR_HANDLER` (`std::function<void( const wxString& msg, BOARD_ITEM* itemA,
/// BOARD_ITEM* itemB, const VECTOR2I& pt )>`): the items as handles, `None` for a null
/// pointer. Passed as `Option<&mut dyn OutlineErrorHandler>` (C++ `OUTLINE_ERROR_HANDLER*`,
/// null = no reporting). Every closure with the four parameters is a handler:
/// `Some(&mut |msg: &str, a, b, pt| ...)`.
pub trait OutlineErrorHandler {
    fn on_error(&mut self, msg: &str, item_a: Option<ItemId>, item_b: Option<ItemId>, pt: Vector2I);
}

impl<F> OutlineErrorHandler for F
where
    F: FnMut(&str, Option<ItemId>, Option<ItemId>, Vector2I),
{
    fn on_error(
        &mut self,
        msg: &str,
        item_a: Option<ItemId>,
        item_b: Option<ItemId>,
        pt: Vector2I,
    ) {
        self(msg, item_a, item_b, pt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// New code, no C++ test corresponds: a closure and a struct are both handlers.
    #[test]
    fn closures_are_outline_error_handlers() {
        fn report(handler: Option<&mut dyn OutlineErrorHandler>) {
            if let Some(handler) = handler {
                handler.on_error("gap", None, None, Vector2I::new(1, 2));
            }
        }

        let mut seen = Vec::new();
        report(Some(&mut |msg: &str,
                          _a: Option<ItemId>,
                          _b: Option<ItemId>,
                          pt: Vector2I| {
            seen.push((msg.to_string(), pt));
        }));
        report(None);
        assert_eq!(seen, vec![("gap".to_string(), Vector2I::new(1, 2))]);
    }
}
