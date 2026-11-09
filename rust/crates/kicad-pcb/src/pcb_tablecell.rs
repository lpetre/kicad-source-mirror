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

//! Port of `pcbnew/pcb_tablecell.h` and `pcbnew/pcb_tablecell.cpp` (`PCB_TABLECELL`).
//!
//! Placeholder written by M0a; the type package M0c fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16).

use crate::ctx::BoardCtx;
use crate::eda_shape::EdaShape;
use crate::eda_text::EdaText;
use crate::ids::ItemId;
use crate::item::BoardItemBase;
use crate::item::ConnectedItemBase;
use crate::pcb_textbox::PcbTextbox;
use crate::traits::BoardConnectedItem;
use crate::traits::BoardItemMut;
use crate::traits::EdaShapeItem;
use crate::traits::EdaTextItem;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `PCB_TABLECELL`.
///
/// PLACEHOLDER (M0a): M0c replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct PcbTablecell {
    /// PCB_TABLECELL : PCB_TEXTBOX. `parent == ParentRef::Item(table)`.
    pub textbox: PcbTextbox,
}

impl Invalidate for PcbTablecell {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbTablecell {
    fn base(&self) -> &BoardItemBase {
        &self.textbox.shape.base.item
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.textbox.shape.base.item
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbTablecellT
    }

    fn class_name(&self) -> &'static str {
        "PCB_TABLECELL"
    }

    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.textbox.shape.base)
    }

    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.textbox.shape.base)
    }

    fn parent_group(&self, ctx: &BoardCtx<'_>) -> Option<ItemId> {
        PcbTablecell::parent_group(self, ctx)
    }
}

impl BoardConnectedItem for PcbTablecell {}

impl BoardItemMut for PcbTablecell {}

/// A cell is its own `EDA_TEXT` host (PCB_TABLECELL overrides `GetShownText`), so that
/// `ItemRef::as_eda_text()` and the generic `eda_text` functions dispatch to the cell's
/// overrides and not to the embedded `PcbTextbox`. M0c adds a shim for every `EDA_TEXT`
/// virtual that PCB_TABLECELL or PCB_TEXTBOX overrides, each forwarding to an inherent
/// `PcbTablecell` method.
impl EdaTextItem for PcbTablecell {
    fn eda_text(&self) -> &EdaText {
        &self.textbox.text
    }

    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.textbox.text
    }
}

/// A cell is its own `EDA_SHAPE` host (PCB_TABLECELL overrides `IsFilledForHitTesting`);
/// M0c adds the shims for the `EDA_SHAPE` virtuals that PCB_TABLECELL, PCB_TEXTBOX or
/// PCB_SHAPE override.
impl EdaShapeItem for PcbTablecell {
    fn eda_shape(&self) -> &EdaShape {
        &self.textbox.shape.shape
    }

    fn eda_shape_mut(&mut self) -> &mut EdaShape {
        &mut self.textbox.shape.shape
    }
}

impl PcbTablecell {
    /// `PCB_TABLECELL::GetParentGroup()`: the group of the parent table (a cell is never a
    /// group member itself).
    pub fn parent_group(&self, ctx: &BoardCtx<'_>) -> Option<ItemId> {
        let table = ctx.board.get(self.textbox.shape.base.item.parent.item()?)?;
        table.as_dyn().parent_group(ctx)
    }
}
