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

//! Port of the text-variable, property, variant and embedded-file functions of
//! `pcbnew/board.cpp`. Package M22; M0d wrote the signatures (`board-model-types.md` 18).

use crate::board::Board;
use crate::component_class::ComponentClassManager;
use crate::embedded_files::EmbeddedFiles;

impl Board {
    /// `BOARD::HasVariant`.
    pub fn has_variant(&self, name: &str) -> bool {
        crate::port_pending!("BOARD::HasVariant", name)
    }

    /// `BOARD::AddVariant`.
    pub fn add_variant(&mut self, name: &str) {
        crate::port_pending!("BOARD::AddVariant", name)
    }

    /// `BOARD::DeleteVariant`.
    pub fn delete_variant(&mut self, name: &str) {
        crate::port_pending!("BOARD::DeleteVariant", name)
    }

    /// `BOARD::RenameVariant`.
    pub fn rename_variant(&mut self, old: &str, new: &str) {
        crate::port_pending!("BOARD::RenameVariant", old, new)
    }

    /// `BOARD::GetVariantDescription`.
    pub fn get_variant_description(&self, name: &str) -> String {
        crate::port_pending!("BOARD::GetVariantDescription", name)
    }

    /// `BOARD::SynchronizeProperties`.
    pub fn synchronize_properties(&mut self) {
        crate::port_pending!("BOARD::SynchronizeProperties")
    }

    /// `BOARD::ResolveTextVar( token, variant, depth )`: title block, board properties,
    /// built-ins, project text variables.
    pub fn resolve_text_var(&self, token: &mut String, variant: &str, depth: i32) -> bool {
        crate::port_pending!("BOARD::ResolveTextVar", token, variant, depth)
    }

    /// `BOARD::ConvertCrossReferencesToKIIDs`.
    pub fn convert_cross_references_to_kiids(&self, source: &str) -> String {
        crate::port_pending!("BOARD::ConvertCrossReferencesToKIIDs", source)
    }

    /// `BOARD::ConvertKIIDsToCrossReferences`.
    pub fn convert_kiids_to_cross_references(&self, source: &str) -> String {
        crate::port_pending!("BOARD::ConvertKIIDsToCrossReferences", source)
    }

    /// `BOARD::GetEmbeddedFiles`.
    pub fn embedded_files(&self) -> &EmbeddedFiles {
        &self.embedded_files
    }

    /// `BOARD::FixupEmbeddedData`.
    pub fn fixup_embedded_data(&mut self) {
        crate::port_pending!("BOARD::FixupEmbeddedData")
    }

    /// `BOARD::GetComponentClassManager`.
    pub fn component_class_manager(&self) -> &ComponentClassManager {
        &self.component_class_manager
    }
}
