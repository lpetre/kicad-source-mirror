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
//! `pcbnew/board.cpp` (`ResolveTextVar`, `GetContextualTextVars`, the variant registry,
//! `SynchronizeProperties`, the cross-reference converters, `FixupEmbeddedData`) and the
//! board's use of `TITLE_BLOCK::TextVarResolver` / `PROJECT::TextVarResolver`
//! (`common/title_block.cpp`, `common/project.cpp`). Package M22.

use std::sync::Arc;

use kicad_core::Kiid;
use kicad_core::case_insensitive_map::{CaseInsensitiveKey, case_insensitive_cmp};
use kicad_core::string_utils::get_default_variant_name;
use kicad_core::text_vars::{ResolutionContext, expand_text_vars};
use kicad_core::title_block::TitleBlockClock;

use crate::board::{Board, BoardProject};
use crate::component_class::ComponentClassManager;
use crate::ctx::BoardCtx;
use crate::embedded_files::EmbeddedFiles;
use crate::ids::{FootprintId, ItemKind};

/// `aName.CmpNoCase( aOther ) == 0`.
fn same_no_case(a: &str, b: &str) -> bool {
    case_insensitive_cmp(a, b).is_eq()
}

/// `FindVariantNameCaseInsensitive( aNames, aVariantName )` (`board.cpp`, static): the stored
/// spelling of the variant, empty when there is none.
fn find_variant_name_case_insensitive(names: &[String], variant_name: &str) -> String {
    for name in names {
        if same_no_case(name, variant_name) {
            return name.clone();
        }
    }

    String::new()
}

/// `wxString::BeforeFirst( ':', &remainder )`: the text before the first `:` and the text
/// after it (the whole string and an empty remainder when there is none).
fn before_first_colon(token: &str) -> (String, String) {
    match token.split_once(':') {
        Some((before, after)) => (before.to_string(), after.to_string()),
        None => (token.to_string(), String::new()),
    }
}

/// `wxFileName( aPath ).GetFullName()`: the last path component (Unix separators, the
/// reference platform).
fn file_full_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or("").to_string()
}

/// `wxFileName( aPath ).GetName()`: the last path component without its extension.
fn file_name_stem(path: &str) -> String {
    let full = file_full_name(path);

    match full.rfind('.') {
        Some(0) | None => full,
        Some(dot) => full[..dot].to_string(),
    }
}

/// `PROJECT::TextVarResolver( aToken )` for the board's project.
///
/// * `PROJECTNAME`: `BoardProject` has no project name; the name of the board file is used,
///   which is what `kicad-cli` binds (`<board dir>/<board name>.kicad_pro`).
/// * `CURRENT_DATE`, `CURRENT_TIME_*`: from `clock` (no wall clock in library code).
/// * `VCSHASH` / `VCSSHORTHASH`: there is no git access in library code; they resolve to an
///   empty string, what `GetCurrentHash` returns for a project outside a repository.
fn project_text_var_resolver(
    project: &BoardProject,
    board_file_name: &str,
    clock: &TitleBlockClock,
    token: &mut String,
) -> bool {
    match token.as_str() {
        "PROJECTNAME" => {
            *token = file_name_stem(board_file_name);
            true
        }
        "CURRENT_DATE" => {
            *token = clock.current_date.clone();
            true
        }
        "CURRENT_TIME_HH_MM_SS" => {
            *token = clock.current_time_hhmmss.clone();
            true
        }
        "CURRENT_TIME_LOCALE" => {
            *token = clock.current_time_locale.clone();
            true
        }
        "VCSHASH" | "VCSSHORTHASH" => {
            *token = String::new();
            true
        }
        _ => match project.text_vars.get(token.as_str()) {
            Some(value) => {
                *token = value.clone();
                true
            }
            None => false,
        },
    }
}

impl Board {
    /// The clock text variables resolve against. Library code has no wall clock: the dates and
    /// times resolve to the `TitleBlockClock::default()` values (empty) until the board carries
    /// a host-provided clock.
    fn text_var_clock(&self) -> TitleBlockClock {
        TitleBlockClock::default()
    }

    /// `PROJECT::TextVarResolver` of the board's project (false without a project).
    fn project_text_var_resolver(&self, token: &mut String) -> bool {
        match &self.project {
            Some(project) => {
                project_text_var_resolver(project, &self.file_name, &self.text_var_clock(), token)
            }
            None => false,
        }
    }

    /// `TITLE_BLOCK::TextVarResolver( aToken, m_project, INTERNAL )` of the board's title block:
    /// `kicad_core::TitleBlock::text_var_resolver` plus the project step it leaves to the
    /// caller (the value is expanded against the project, and a value that is still
    /// `${TOKEN}` afterwards counts as unresolved).
    fn title_block_text_var_resolver(&self, token: &mut String) -> bool {
        let original_token = token.clone();
        let resolved = self.titles.text_var_resolver(token, &self.text_var_clock());

        if self.project.is_none() {
            return resolved;
        }

        // The core resolver returns false either for a token that is not a title block
        // variable (token unchanged) or for a value equal to "${TOKEN}" (token replaced by it);
        // with a project C++ expands that value before the check.
        if !resolved && *token == original_token {
            return false;
        }

        // A value equal to "CURRENT_DATE" was already replaced by the date, which has no
        // variable to expand.
        let resolver = |t: &mut String| self.project_text_var_resolver(t);
        *token = expand_text_vars(token, Some(&resolver), ResolutionContext::Internal);

        // This is the default fallback, so don't claim we resolved it
        if *token == format!("${{{original_token}}}") {
            return false;
        }

        true
    }

    /// `FOOTPRINT::ResolveTextVar( token, variant, depth )` on a stored footprint; `false`
    /// for a stale handle.
    fn footprint_resolve_text_var(
        &self,
        footprint: FootprintId,
        token: &mut String,
        variant: &str,
        depth: i32,
    ) -> bool {
        let ctx = BoardCtx::bare(self);

        self.footprint(footprint)
            .is_some_and(|fp| fp.resolve_text_var_for_variant(&ctx, token, variant, depth))
    }

    /// `FOOTPRINT::GetReference()` of a footprint of this board.
    fn footprint_reference(&self, footprint: FootprintId) -> String {
        let ctx = BoardCtx::bare(self);
        self[footprint].reference(&ctx)
    }

    /// `BOARD::HasVariant` (case-insensitive).
    pub fn has_variant(&self, name: &str) -> bool {
        !find_variant_name_case_insensitive(&self.variant_names, name).is_empty()
    }

    /// `BOARD::AddVariant`: ignored for an empty name, the default variant name and an
    /// existing variant.
    pub fn add_variant(&mut self, name: &str) {
        if name.is_empty()
            || same_no_case(name, &get_default_variant_name())
            || self.has_variant(name)
        {
            return;
        }

        self.variant_names.push(name.to_string());
    }

    /// `BOARD::DeleteVariant`: removes the variant (matched case-insensitively), its
    /// description, the current-variant selection if it was that one, and the variant of
    /// every footprint.
    pub fn delete_variant(&mut self, name: &str) {
        if name.is_empty() || same_no_case(name, &get_default_variant_name()) {
            return;
        }

        let Some(pos) = self
            .variant_names
            .iter()
            .position(|n| same_no_case(n, name))
        else {
            return;
        };

        let actual_name = self.variant_names.remove(pos);
        self.variant_descriptions.remove(&actual_name);

        // Clear current variant if it was the deleted one
        if same_no_case(&self.current_variant, name) {
            self.current_variant.clear();
        }

        // Remove variant from all footprints
        for fp in self.footprint_ids() {
            self.footprint_delete_variant(fp, &actual_name);
        }

        self.increment_time_stamp();
    }

    /// `BOARD::RenameVariant`.
    pub fn rename_variant(&mut self, old: &str, new: &str) {
        if new.is_empty() || same_no_case(new, &get_default_variant_name()) {
            return;
        }

        let Some(pos) = self.variant_names.iter().position(|n| same_no_case(n, old)) else {
            return;
        };

        let actual_old_name = self.variant_names[pos].clone();

        // Check if new name already exists (case-insensitive) and isn't the same variant
        let existing_name = find_variant_name_case_insensitive(&self.variant_names, new);

        if !existing_name.is_empty() && !same_no_case(&existing_name, &actual_old_name) {
            return;
        }

        if actual_old_name == new {
            return;
        }

        self.variant_names[pos] = new.to_string();

        // Transfer description
        if let Some(description) = self.variant_descriptions.remove(&actual_old_name) {
            if !description.is_empty() {
                self.variant_descriptions
                    .insert(new.to_string(), description);
            }
        }

        // Update current variant if it was the renamed one
        if same_no_case(&self.current_variant, old) {
            self.current_variant = new.to_string();
        }

        // Rename variant in all footprints
        for fp in self.footprint_ids() {
            self.footprint_rename_variant(fp, &actual_old_name, new);
        }

        self.increment_time_stamp();
    }

    /// `FOOTPRINT::DeleteVariant` (`m_variants.erase( aVariantName )`, case-insensitive map),
    /// inlined: the footprint package (M19) has no Rust counterpart yet.
    fn footprint_delete_variant(&mut self, fp: FootprintId, name: &str) {
        if let Some(footprint) = self.footprints.get_mut(fp) {
            footprint
                .variants
                .remove(&CaseInsensitiveKey(name.to_string()));
        }
    }

    /// `FOOTPRINT::RenameVariant`, inlined like `footprint_delete_variant`.
    fn footprint_rename_variant(&mut self, fp: FootprintId, old: &str, new: &str) {
        let Some(footprint) = self.footprints.get_mut(fp) else {
            return;
        };

        if new.is_empty() || same_no_case(new, &get_default_variant_name()) {
            return;
        }

        let old_key = CaseInsensitiveKey(old.to_string());
        let new_key = CaseInsensitiveKey(new.to_string());

        let Some((stored_old, _)) = footprint.variants.get_key_value(&old_key) else {
            return;
        };

        let stored_old = stored_old.as_str().to_string();

        // existingIt != it: a different entry under the new name
        if footprint.variants.contains_key(&new_key) && !same_no_case(&stored_old, new) {
            return;
        }

        if stored_old == new {
            return;
        }

        if let Some(mut variant) = footprint.variants.remove(&old_key) {
            variant.name = new.to_string();
            // emplace: keeps an existing entry (impossible here, it was the same key)
            footprint.variants.entry(new_key).or_insert(variant);
        }
    }

    /// `BOARD::SetCurrentVariant`: the stored spelling of an existing variant, else none (the
    /// default variant). The text-variable dependency tracker of the GUI has no counterpart.
    pub fn set_current_variant(&mut self, variant: &str) {
        if variant.is_empty() || same_no_case(variant, &get_default_variant_name()) {
            self.current_variant.clear();
        } else {
            let actual_name = find_variant_name_case_insensitive(&self.variant_names, variant);

            if actual_name.is_empty() {
                self.current_variant.clear();
            } else {
                self.current_variant = actual_name;
            }
        }
    }

    /// `BOARD::GetCurrentVariant`.
    pub fn get_current_variant(&self) -> &str {
        &self.current_variant
    }

    /// `BOARD::GetVariantDescription`: empty for the default variant and unknown variants.
    pub fn get_variant_description(&self, name: &str) -> String {
        if name.is_empty() || same_no_case(name, &get_default_variant_name()) {
            return String::new();
        }

        let actual_name = find_variant_name_case_insensitive(&self.variant_names, name);

        if actual_name.is_empty() {
            return String::new();
        }

        self.variant_descriptions
            .get(&actual_name)
            .cloned()
            .unwrap_or_default()
    }

    /// `BOARD::SetVariantDescription`: an empty description removes it.
    pub fn set_variant_description(&mut self, name: &str, description: &str) {
        if name.is_empty() || same_no_case(name, &get_default_variant_name()) {
            return;
        }

        let actual_name = find_variant_name_case_insensitive(&self.variant_names, name);

        if actual_name.is_empty() {
            return;
        }

        if description.is_empty() {
            self.variant_descriptions.remove(&actual_name);
        } else {
            self.variant_descriptions
                .insert(actual_name, description.to_string());
        }
    }

    /// `BOARD::SynchronizeProperties`: the project text variables become the board
    /// properties. Every `BoardProject` stands for a real project (the null project of C++ is
    /// `None`).
    pub fn synchronize_properties(&mut self) {
        if let Some(project) = &self.project {
            self.properties = project.text_vars.clone();
        }
    }

    /// `BOARD::GetContextualTextVars`: the variable names a board text can use.
    pub fn get_contextual_text_vars(&self, vars: &mut Vec<String>) {
        let mut add = |var: &str| {
            if !vars.iter().any(|v| v == var) {
                vars.push(var.to_string());
            }
        };

        add("LAYER");
        add("FILENAME");
        add("FILEPATH");
        add("PROJECTNAME");
        add("DRC_ERROR <message_text>");
        add("DRC_WARNING <message_text>");
        add("VARIANT");
        add("VARIANT_DESC");
        add("DRILL_OPERATIONS");
        add("DRILL_SITES");
        add("DRILL_GROUPS");

        kicad_core::TitleBlock::get_contextual_text_vars(vars);

        if let Some(project) = &self.project {
            for name in project.text_vars.keys() {
                if !vars.iter().any(|v| v == name) {
                    vars.push(name.clone());
                }
            }
        }
    }

    /// `BOARD::ResolveTextVar( token, aDepth )`: with the current variant.
    pub fn resolve_text_var_current(&self, token: &mut String, depth: i32) -> bool {
        self.resolve_text_var(token, &self.current_variant, depth)
    }

    /// `BOARD::ResolveTextVar( token, variant, depth )`: cross references
    /// (`<uuid or reference>:<var>`), the built-ins `FILENAME`, `FILEPATH`, `VARIANT`,
    /// `VARIANT_DESC`, `PROJECTNAME`, `DRILL_*`, then the title block, the project's text
    /// variables and finally the board properties.
    pub fn resolve_text_var(&self, token: &mut String, variant: &str, depth: i32) -> bool {
        if token.contains(':') {
            let (reference, remainder) = before_first_colon(token);
            let ref_item = Kiid::parse(&reference).and_then(|uuid| self.resolve_item(&uuid));

            if let Some(item) = ref_item {
                if item.kind() == ItemKind::Footprint {
                    if let Some(fp) = item.typed::<crate::footprint::Footprint>() {
                        let mut remainder = remainder.clone();

                        if self.footprint_resolve_text_var(fp, &mut remainder, variant, depth + 1) {
                            *token = remainder;
                        } else {
                            *token = format!(
                                "<Unresolved: {}:{}>",
                                self.footprint_reference(fp),
                                remainder
                            );
                        }

                        return true;
                    }
                }
            }

            // If UUID resolution failed, try to resolve by reference designator
            // This handles typing ${U1:VALUE} directly without save/reload
            if ref_item.is_none() {
                for fp in self.footprint_ids() {
                    if same_no_case(&self.footprint_reference(fp), &reference) {
                        let mut remainder = remainder.clone();

                        if self.footprint_resolve_text_var(fp, &mut remainder, variant, depth + 1) {
                            *token = remainder;
                        } else {
                            *token = format!("<Unresolved: {reference}:{remainder}>");
                        }

                        return true;
                    }
                }

                *token = format!("<Unresolved: {reference}:{remainder}>");
                return true;
            }
        }

        match token.as_str() {
            "FILENAME" => {
                *token = file_full_name(&self.file_name);
                return true;
            }
            "FILEPATH" => {
                // wxFileName( GetFileName() ).GetFullPath()
                *token = self.file_name.clone();
                return true;
            }
            "VARIANT" => {
                *token = variant.to_string();
                return true;
            }
            "VARIANT_DESC" => {
                *token = self.get_variant_description(variant);
                return true;
            }
            "PROJECTNAME" if self.project.is_some() => {
                *token = file_name_stem(&self.file_name);
                return true;
            }
            "DRILL_OPERATIONS" | "DRILL_SITES" | "DRILL_GROUPS" => {
                // Cached, because this resolves on every redraw of every text item and the model
                // walks every hole on every span
                crate::port_pending!("BOARD::DrillSymbolCache (DRILL_CHART_TOTALS)", token)
            }
            _ => {}
        }

        let var = token.clone();

        if self.title_block_text_var_resolver(token) {
            return true;
        }

        // Resolve from the project's live text variables before the board's cached properties so
        // changes made outside the board (schematic, project settings) are always reflected.
        if self.project_text_var_resolver(token) {
            return true;
        }

        // Fall back to the board's cached properties for backward compatibility with boards that
        // may carry properties not present in the project.
        if let Some(value) = self.properties.get(&var) {
            *token = value.clone();
            return true;
        }

        false
    }

    /// `BOARD::ConvertCrossReferencesToKIIDs`: `${REF:VAR}` becomes `${<uuid>:VAR}` for a
    /// footprint whose reference matches (case-insensitively) and which resolves `VAR`.
    /// Escaped expressions (`\${...}`, `\@{...}`) are copied verbatim.
    pub fn convert_cross_references_to_kiids(&self, source: &str) -> String {
        self.convert_cross_references(source, |token| {
            let (reference, remainder) = before_first_colon(token);

            for fp in self.footprint_ids() {
                if same_no_case(&self.footprint_reference(fp), &reference) {
                    let mut test = remainder.clone();

                    if self.footprint_resolve_text_var(fp, &mut test, &self.current_variant, 0) {
                        *token = format!("{}:{}", self[fp].base.uuid.as_string(), remainder);
                    }

                    break;
                }
            }
        })
    }

    /// `BOARD::ConvertKIIDsToCrossReferences`: `${<uuid>:VAR}` of a footprint becomes
    /// `${REF:VAR}`. Escaped expressions are copied verbatim.
    pub fn convert_kiids_to_cross_references(&self, source: &str) -> String {
        self.convert_cross_references(source, |token| {
            let (reference, remainder) = before_first_colon(token);
            let ref_item = Kiid::parse(&reference).and_then(|uuid| self.resolve_item(&uuid));

            if let Some(fp) = ref_item.and_then(|item| item.typed::<crate::footprint::Footprint>())
            {
                *token = format!("{}:{}", self.footprint_reference(fp), remainder);
            }
        })
    }

    /// The scanner shared by `ConvertCrossReferencesToKIIDs` and
    /// `ConvertKIIDsToCrossReferences` (identical in C++ except for the token rewrite, which
    /// `rewrite` does for a token containing `:`). Works on characters like `wxString`.
    fn convert_cross_references(
        &self,
        source: &str,
        mut rewrite: impl FnMut(&mut String),
    ) -> String {
        let src: Vec<char> = source.chars().collect();
        let source_len = src.len();
        let mut newbuf = String::new();
        let mut i = 0usize;

        while i < source_len {
            // Check for escaped expressions: \${ or \@{
            // These should be copied verbatim without any conversion
            if src[i] == '\\'
                && i + 2 < source_len
                && src[i + 2] == '{'
                && (src[i + 1] == '$' || src[i + 1] == '@')
            {
                // Copy the escape sequence and the entire escaped expression
                newbuf.push(src[i]); // backslash
                newbuf.push(src[i + 1]); // $ or @
                newbuf.push(src[i + 2]); // {
                i += 2;

                // Find and copy everything until the matching closing brace
                let mut brace_depth = 1;
                i += 1;

                while i < source_len && brace_depth > 0 {
                    if src[i] == '{' {
                        brace_depth += 1;
                    } else if src[i] == '}' {
                        brace_depth -= 1;
                    }

                    newbuf.push(src[i]);
                    i += 1;
                }

                // (C++: i-- then the loop's ++i)
                continue;
            }

            if src[i] == '$' && i + 1 < source_len && src[i + 1] == '{' {
                let mut token = String::new();
                let mut is_cross_ref = false;

                i += 2;

                while i < source_len {
                    if src[i] == '}' {
                        break;
                    }

                    if src[i] == ':' {
                        is_cross_ref = true;
                    }

                    token.push(src[i]);
                    i += 1;
                }

                if is_cross_ref {
                    rewrite(&mut token);
                }

                newbuf.push_str("${");
                newbuf.push_str(&token);
                newbuf.push('}');
            } else {
                newbuf.push(src[i]);
            }

            i += 1;
        }

        newbuf
    }

    /// `BOARD::GetEmbeddedFiles`.
    pub fn embedded_files(&self) -> &EmbeddedFiles {
        &self.embedded_files
    }

    /// `BOARD::FixupEmbeddedData`: every footprint's embedded file that the board also embeds
    /// takes the board's data (`RunOnNestedEmbeddedFiles`: footprints in board order).
    pub fn fixup_embedded_data(&mut self) {
        let board_files = &self.embedded_files;

        for &fp in &self.footprint_list {
            let Some(footprint) = self.footprints.get_mut(fp) else {
                continue;
            };

            for (filename, embedded_file) in footprint.embedded_files.files.iter_mut() {
                if let Some(file) = board_files.get_embedded_file(filename) {
                    let target = Arc::make_mut(embedded_file);
                    target.compressed_encoded_data = file.compressed_encoded_data.clone();
                    target.decompressed_data = file.decompressed_data.clone();
                    target.data_hash = file.data_hash.clone();
                    target.is_valid = file.is_valid;
                }
            }
        }
    }

    /// `BOARD::GetComponentClassManager`.
    pub fn component_class_manager(&self) -> &ComponentClassManager {
        &self.component_class_manager
    }
}

/// Package M22 tests. Ports of `qa/tests/pcbnew/test_text_variable_resolution.cpp` (the
/// `TextVariableResolution` suite; `PcbTextResolvesProjectVars` needs `PCB_TEXT::GetShownText`,
/// package M10) and of the board cases of `qa/tests/pcbnew/test_variant.cpp`.
#[cfg(test)]
mod m22_tests {
    use std::collections::BTreeMap;

    use super::*;

    /// `TEXT_VAR_RESOLUTION_FIXTURE`: a board bound to an (empty) project.
    fn board_with_project() -> Board {
        let mut board = Board::default();
        board.project = Some(BoardProject::default());
        board
    }

    fn project_vars(board: &mut Board) -> &mut BTreeMap<String, String> {
        &mut board.project.as_mut().unwrap().text_vars
    }

    fn resolve(board: &Board, token: &str) -> (bool, String) {
        let mut t = token.to_string();
        let ok = board.resolve_text_var_current(&mut t, 0);
        (ok, t)
    }

    #[test]
    fn project_text_vars_take_precedence() {
        let mut board = board_with_project();
        project_vars(&mut board).insert("MYVAR".into(), "project_value".into());
        board
            .properties
            .insert("MYVAR".into(), "cached_board_value".into());

        assert_eq!(resolve(&board, "MYVAR"), (true, "project_value".into()));
    }

    #[test]
    fn project_text_vars_update_dynamically() {
        let mut board = board_with_project();
        project_vars(&mut board).insert("MY_REVISION".into(), "1.0".into());
        assert_eq!(resolve(&board, "MY_REVISION"), (true, "1.0".into()));

        project_vars(&mut board).insert("MY_REVISION".into(), "2.0".into());
        assert_eq!(resolve(&board, "MY_REVISION"), (true, "2.0".into()));
    }

    #[test]
    fn title_block_vars_take_precedence() {
        let mut board = board_with_project();
        board.titles.set_title("board_title");
        project_vars(&mut board).insert("TITLE".into(), "project_title".into());
        board
            .properties
            .insert("TITLE".into(), "cached_board_value".into());

        assert_eq!(resolve(&board, "TITLE"), (true, "board_title".into()));
    }

    #[test]
    fn board_properties_fallback() {
        let mut board = board_with_project();
        board
            .properties
            .insert("LEGACY_VAR".into(), "legacy_value".into());

        assert_eq!(resolve(&board, "LEGACY_VAR"), (true, "legacy_value".into()));
        assert!(!resolve(&board, "UNKNOWN").0);
    }

    /// `TITLE_BLOCK::TextVarResolver` with a project expands the value against the project, and
    /// a value that stays `${TOKEN}` counts as unresolved.
    #[test]
    fn title_block_value_expanded_against_project() {
        let mut board = board_with_project();
        board.titles.set_revision("r${REV}");
        project_vars(&mut board).insert("REV".into(), "7".into());
        assert_eq!(resolve(&board, "REVISION"), (true, "r7".into()));

        // "${COMPANY}" expands to itself through the project: unresolved, then the project and
        // the properties are asked.
        board.titles.set_company("${COMPANY}");
        assert!(!resolve(&board, "COMPANY").0);
        project_vars(&mut board).insert("COMPANY".into(), "ACME".into());
        assert_eq!(resolve(&board, "COMPANY"), (true, "ACME".into()));

        // Without a project the value is used as is.
        let mut bare = Board::default();
        bare.titles.set_revision("r${REV}");
        assert_eq!(resolve(&bare, "REVISION"), (true, "r${REV}".into()));
    }

    #[test]
    fn built_in_variables() {
        let mut board = Board::default();
        board.file_name = "/tmp/dir/my_board.kicad_pcb".into();

        assert_eq!(
            resolve(&board, "FILENAME"),
            (true, "my_board.kicad_pcb".into())
        );
        assert_eq!(
            resolve(&board, "FILEPATH"),
            (true, "/tmp/dir/my_board.kicad_pcb".into())
        );
        // PROJECTNAME needs a project.
        assert!(!resolve(&board, "PROJECTNAME").0);
        board.project = Some(BoardProject::default());
        assert_eq!(resolve(&board, "PROJECTNAME"), (true, "my_board".into()));

        // A cross reference to nothing on the board.
        assert_eq!(
            resolve(&board, "R99:VALUE"),
            (true, "<Unresolved: R99:VALUE>".into())
        );

        let mut vars = Vec::new();
        board.get_contextual_text_vars(&mut vars);
        assert_eq!(vars[0], "LAYER");
        assert!(vars.iter().any(|v| v == "COMMENT9"));
        assert_eq!(vars.iter().filter(|v| *v == "VARIANT").count(), 1);
    }

    /// `BoardVariantRegistry`, `VariantCaseInsensitivity` (board part).
    #[test]
    fn board_variant_registry() {
        let mut board = Board::default();

        // Initially no variants
        assert!(board.variant_names.is_empty());
        assert!(board.get_current_variant().is_empty());

        // Add a variant
        board.add_variant("Production");
        assert!(board.has_variant("Production"));
        assert_eq!(board.variant_names.len(), 1);

        // Add another variant
        board.add_variant("Debug");
        assert!(board.has_variant("Debug"));
        assert_eq!(board.variant_names.len(), 2);

        // Test case insensitivity
        assert!(board.has_variant("production"));
        assert!(board.has_variant("PRODUCTION"));
        assert!(board.has_variant("PrOdUcTiOn"));

        // Duplicates and the default name are ignored.
        board.add_variant("DEBUG");
        board.add_variant("< default >");
        board.add_variant("");
        assert_eq!(board.variant_names.len(), 2);

        // Set current variant
        board.set_current_variant("Production");
        assert_eq!(board.get_current_variant(), "Production");
        board.set_current_variant("production");
        assert_eq!(board.get_current_variant(), "Production");
        board.set_current_variant("nope");
        assert!(board.get_current_variant().is_empty());
        board.set_current_variant("Production");

        // Set variant description
        board.set_variant_description("Production", "Standard production build");
        assert_eq!(
            board.get_variant_description("Production"),
            "Standard production build"
        );

        // Delete a variant
        board.delete_variant("Debug");
        assert!(!board.has_variant("Debug"));
        assert_eq!(board.variant_names.len(), 1);
    }

    /// `BoardVariantTextVars`.
    #[test]
    fn board_variant_text_vars() {
        let mut board = Board::default();

        board.add_variant("Production");
        board.set_variant_description("Production", "Production build");
        board.set_current_variant("Production");

        assert_eq!(resolve(&board, "VARIANT"), (true, "Production".into()));
        assert_eq!(
            resolve(&board, "VARIANT_DESC"),
            (true, "Production build".into())
        );

        board.set_current_variant("");
        assert_eq!(resolve(&board, "VARIANT"), (true, String::new()));
    }

    /// `VariantDeletionClearsRegistry`.
    #[test]
    fn variant_deletion_clears_registry() {
        let mut board = Board::default();

        board.add_variant("Variant1");
        board.add_variant("Variant2");
        board.set_variant_description("Variant1", "Description 1");
        board.set_current_variant("Variant1");

        assert_eq!(board.variant_names.len(), 2);
        assert!(board.has_variant("Variant1"));

        // Delete the variant
        board.delete_variant("Variant1");

        // Verify it's gone
        assert_eq!(board.variant_names.len(), 1);
        assert!(!board.has_variant("Variant1"));
        assert!(board.has_variant("Variant2"));
        assert!(board.variant_descriptions.is_empty());

        // Current variant should be cleared since we deleted the current one
        assert!(board.get_current_variant().is_empty());
    }

    /// `RenameVariantPreservesData`, plus the footprint side (`FOOTPRINT::RenameVariant` /
    /// `DeleteVariant`).
    #[test]
    fn rename_variant_preserves_data() {
        let mut fx = crate::test_support::rotated_flipped_scaled();
        let board = &mut fx.board;
        let fp = fx.footprint;
        let mut variant = crate::footprint::FootprintVariant::new("OldName");
        variant.dnp = true;
        board
            .footprints
            .get_mut(fp)
            .unwrap()
            .variants
            .insert(CaseInsensitiveKey("OldName".into()), variant);

        board.add_variant("OldName");
        board.set_variant_description("OldName", "Test description");
        board.set_current_variant("OldName");

        // Rename the variant
        board.rename_variant("oldname", "NewName");

        // Old name should be gone
        assert!(!board.has_variant("OldName"));

        // New name should exist with same properties
        assert!(board.has_variant("NewName"));
        assert_eq!(board.get_variant_description("NewName"), "Test description");

        // Current variant should be updated if it was the renamed one
        assert_eq!(board.get_current_variant(), "NewName");

        let fpv = board
            .footprints
            .get(fp)
            .unwrap()
            .variant("newname")
            .unwrap();
        assert_eq!(fpv.name, "NewName");
        assert!(fpv.dnp);

        board.delete_variant("NEWNAME");
        assert!(board.footprints.get(fp).unwrap().variants.is_empty());
        board.check_invariants().unwrap();
    }

    #[test]
    fn synchronize_properties_copies_project_vars() {
        let mut board = Board::default();
        board.properties.insert("A".into(), "1".into());
        board.synchronize_properties();
        assert_eq!(board.properties.len(), 1);

        board.project = Some(BoardProject::default());
        project_vars(&mut board).insert("B".into(), "2".into());
        board.synchronize_properties();
        let vars = project_vars(&mut board).clone();
        assert_eq!(board.properties, vars);
    }

    #[test]
    fn cross_reference_conversion_without_footprints() {
        let board = Board::default();
        let src = "a ${R1:VALUE} \\${X:Y} ${TITLE} \\@{1+{2}} z";
        assert_eq!(board.convert_cross_references_to_kiids(src), src);
        assert_eq!(board.convert_kiids_to_cross_references(src), src);
        // An unterminated reference is closed, as in C++.
        assert_eq!(board.convert_kiids_to_cross_references("${A:B"), "${A:B}");
        assert_eq!(board.convert_kiids_to_cross_references("x$"), "x$");
    }
}
