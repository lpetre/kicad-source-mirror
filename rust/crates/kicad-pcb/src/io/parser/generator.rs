// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of the generator and constraint parts of
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (package PCBIO-13):
//! `parseGENERATOR`, `parseGENERATOR_templates`, `parseCONSTRAINT`, `resolveConstraints`
//! (P:8444-8486, 8579-8948).
//!
//! Generators and constraints are only *recorded* while the file is read ([`GeneratorInfo`] /
//! [`ConstraintInfo`]); `finishItem` resolves them once the board or footprint is complete:
//! generators through the group machinery of PCBIO-09 (`resolve_groups`, which calls
//! `generator_from_info`: `CreateFromType`, `SetLayer`, `SetProperties`, `SetTemplateItem`),
//! constraints through [`PcbParser::resolve_constraints`] below.

use std::collections::{BTreeMap, HashMap};

use kicad_core::{Kiid, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::{ShapeLineChain, Vector2I};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use super::{ConstraintInfo, GeneratorInfo, GroupInfo};
use crate::board::AddMode;
use crate::ctx::BoardCtx;
use crate::ids::{FootprintId, ItemId, ItemKind};
use crate::item::{ParentRef, RecurseMode};
use crate::item_ref::OwnedItem;
use crate::pcb_constraint::{
    ConstraintAnchor, ConstraintMember, PcbConstraint, PcbConstraintType,
    constraint_anchor_from_token, constraint_type_from_token, constraint_value_is_length,
};
use crate::pcb_generator::{
    GENERATOR_TYPE_TUNING_PATTERN, GENERATOR_TYPE_VIA_STACK, GeneratorProp,
};

impl PcbParser {
    /// Port of `parseGENERATOR( BOARD_ITEM* aParent )`: records a [`GeneratorInfo`] in
    /// `m_generatorInfos`; the generator itself is created by `resolve_groups` (PCBIO-09).
    ///
    /// Ghost tuning patterns and member-less via stacks are dropped again after the parse, as
    /// the C++ does.
    pub(crate) fn parse_generator(&mut self, parent: ParentRef) -> Result<(), IoError> {
        // wxCHECK_RET( CurTok() == T_generated, ... )
        if self.lexer.cur_tok() != T::T_generated {
            debug_assert!(
                false,
                "Cannot parse {} as PCB_GENERATOR.",
                self.lexer.cur_text()
            );
            return Ok(());
        }

        // m_generatorInfos.push_back( GENERATOR_INFO() ): the value-initialised KIID member
        // draws a fresh id (replaced by the mandatory (uuid ...) below).
        let mut gen_info = GeneratorInfo {
            group: GroupInfo {
                parent,
                uuid: Kiid::new(&mut self.board.kiid_generator),
                ..GroupInfo::default()
            },
            // genInfo.layer = F_Cu; genInfo.properties = STRING_ANY_MAP( pcbIUScale.IU_PER_MM )
            layer: PcbLayerId::FCu,
            gen_type: String::new(),
            properties: BTreeMap::new(),
            templates: Vec::new(),
        };

        let result = self.parse_generator_body(&mut gen_info);

        // The C++ fills the vector entry in place: a declaration interrupted by a parse error
        // stays recorded (harmless, the parse fails).
        self.generator_infos.push(gen_info);
        result?;

        // Previous versions had bugs which could save ghost tuning patterns. Ignore them, and
        // ignore member-less microvia stacks already sitting in files for the same reason.
        if let Some(last) = self.generator_infos.last() {
            if last.group.member_uuids.is_empty()
                && (last.gen_type == GENERATOR_TYPE_TUNING_PATTERN
                    || last.gen_type == GENERATOR_TYPE_VIA_STACK)
            {
                self.generator_infos.pop();
            }
        }

        Ok(())
    }

    /// The token loops of `parseGENERATOR` (after the `GENERATOR_INFO` was pushed).
    fn parse_generator_body(&mut self, gen_info: &mut GeneratorInfo) -> Result<(), IoError> {
        self.lexer.need_left()?;
        let mut token = self.lexer.next_tok()?;

        // For formats [20231007, 20231215), 'id' was used instead of 'uuid'
        if token != T::T_uuid && token != T::T_id {
            return Err(self.lexer.expecting(T::T_uuid));
        }

        self.lexer.next_tok()?;
        gen_info.group.uuid = self.cur_str_to_kiid();
        self.lexer.need_right()?;

        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_type => {
                    self.lexer.need_symbol()?;
                    gen_info.gen_type = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_name => {
                    self.lexer.need_symbol()?;
                    gen_info.group.name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_locked => {
                    token = self.lexer.next_tok()?;
                    gen_info.group.locked = token == T::T_yes;
                    self.lexer.need_right()?;
                }

                T::T_layer => {
                    gen_info.layer = self.parse_board_item_layer()?;
                    self.lexer.need_right()?;
                }

                T::T_members => {
                    self.parse_group_members(&mut gen_info.group)?;
                }

                T::T_templates => {
                    self.parse_generator_templates(gen_info)?;
                }

                T::T_custom_property => {
                    self.parse_custom_property_map(&mut gen_info.group.custom_properties)?;
                }

                _ => {
                    self.parse_generator_property(gen_info)?;
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `default:` branch of the `parseGENERATOR` token switch: `(key value)` stored in
    /// the property map with the type of its value (`yes`/`no` bool, number as double, quoted
    /// string, `(xy ..)` point in IU, `(pts ..)` chain, `(cells (ij c r) ..)` integer pairs).
    ///
    /// `STRING_ANY_MAP::emplace`: a key seen a second time keeps its first value.
    fn parse_generator_property(&mut self, gen_info: &mut GeneratorInfo) -> Result<(), IoError> {
        let p_name = self.lexer.from_utf8();
        let tok1 = self.lexer.next_tok()?;

        let value = match tok1 {
            T::T_yes => {
                self.lexer.need_right()?;
                GeneratorProp::Bool(true)
            }

            T::T_no => {
                self.lexer.need_right()?;
                GeneratorProp::Bool(false)
            }

            T::T_NUMBER => {
                let p_value = self.parse_double()?;
                self.lexer.need_right()?;
                GeneratorProp::Number(p_value)
            }

            // Quoted string
            T::T_STRING => {
                let p_value = self.lexer.from_utf8();
                self.lexer.need_right()?;
                GeneratorProp::String(p_value)
            }

            T::T_LEFT => {
                self.lexer.need_symbol()?;
                let tok2 = self.lexer.cur_tok();

                match tok2 {
                    T::T_xy => {
                        let x = self.parse_board_units_str("X coordinate")?;
                        let y = self.parse_board_units_str("Y coordinate")?;

                        self.lexer.need_right()?;
                        self.lexer.need_right()?;
                        GeneratorProp::Vector2I(Vector2I::new(x, y))
                    }

                    T::T_pts => {
                        let mut chain = ShapeLineChain::new();
                        let mut token = self.lexer.next_tok()?;

                        while token != T::T_RIGHT {
                            self.parse_outline_points(&mut chain)?;
                            token = self.lexer.next_tok()?;
                        }

                        self.lexer.need_right()?;
                        GeneratorProp::LineChain(chain)
                    }

                    T::T_cells => {
                        let mut cells: Vec<Vector2I> = Vec::new();
                        let mut token = self.lexer.next_tok()?;

                        while token != T::T_RIGHT {
                            if token != T::T_LEFT {
                                return Err(self.lexer.expecting(T::T_LEFT));
                            }

                            if self.lexer.next_tok()? != T::T_ij {
                                return Err(self.lexer.expecting(T::T_ij));
                            }

                            let x = self.parse_int_expected("column index")?;
                            let y = self.parse_int_expected("row index")?;

                            self.lexer.need_right()?;
                            cells.push(Vector2I::new(x, y));
                            token = self.lexer.next_tok()?;
                        }

                        self.lexer.need_right()?;
                        GeneratorProp::Cells(cells)
                    }

                    _ => return Err(self.lexer.expecting_str("xy, pts or cells")),
                }
            }

            _ => return Err(self.lexer.expecting_str("a number, symbol, string or (")),
        };

        gen_info.properties.entry(p_name).or_insert(value);
        Ok(())
    }

    /// Port of `parseGENERATOR_templates( GENERATOR_INFO& )`: `(templates (template (name "n")
    /// (via ..)) ..)`; a template without an item is dropped, a second item in one template
    /// replaces the first.
    pub(crate) fn parse_generator_templates(
        &mut self,
        gen_info: &mut GeneratorInfo,
    ) -> Result<(), IoError> {
        let mut tok = self.lexer.next_tok()?;

        while tok != T::T_RIGHT {
            if tok != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            if self.lexer.next_tok()? != T::T_template {
                return Err(self.lexer.expecting(T::T_template));
            }

            let mut template_name = String::new();
            let mut parsed: Option<OwnedItem> = None;

            // Parse (template ...
            let mut inner_tok = self.lexer.next_tok()?;

            while inner_tok != T::T_RIGHT {
                if inner_tok != T::T_LEFT {
                    return Err(self.lexer.expecting(T::T_LEFT));
                }

                let inner = self.lexer.next_tok()?;

                match inner {
                    T::T_name => {
                        self.lexer.need_symbol_or_number()?;
                        template_name = self.lexer.from_utf8();
                        self.lexer.need_right()?;
                    }

                    T::T_via => {
                        // A duplicate item in the same (template ...) block replaces the
                        // previous one. parsePCB_VIA consumes the closing T_RIGHT itself.
                        parsed = self.parse_pcb_via()?.map(OwnedItem::Track);
                    }

                    _ => return Err(self.lexer.expecting_str("name or via")),
                }

                inner_tok = self.lexer.next_tok()?;
            }

            if let Some(item) = parsed {
                gen_info.templates.push((template_name, item));
            }

            tok = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parseCONSTRAINT( BOARD_ITEM* aParent )`: records a [`ConstraintInfo`] in
    /// `m_constraintInfos`; the constraint is created by
    /// [`resolve_constraints`](Self::resolve_constraints).
    pub(crate) fn parse_constraint(&mut self, parent: ParentRef) -> Result<(), IoError> {
        // wxCHECK_RET( CurTok() == T_constraint, ... )
        if self.lexer.cur_tok() != T::T_constraint {
            debug_assert!(
                false,
                "Cannot parse {} as PCB_CONSTRAINT.",
                self.lexer.cur_text()
            );
            return Ok(());
        }

        // m_constraintInfos.push_back( CONSTRAINT_INFO() ): the KIID member draws a fresh id.
        let mut info = ConstraintInfo {
            parent,
            uuid: Kiid::new(&mut self.board.kiid_generator),
            type_: PcbConstraintType::Undefined,
            members: Vec::new(),
            value: None,
            driving: true,
            custom_properties: BTreeMap::new(),
        };

        let result = self.parse_constraint_body(&mut info);

        if result.is_ok() {
            // The value is written in mm for length/radius types; convert back to IU now that
            // the type is known (independent of token order within the block).
            if let Some(value) = info.value {
                if constraint_value_is_length(info.type_) {
                    info.value = Some(value * PCB_IU_SCALE.iu_per_mm);
                }
            }
        }

        self.constraint_infos.push(info);
        result
    }

    /// The token loop of `parseCONSTRAINT`.
    fn parse_constraint_body(&mut self, info: &mut ConstraintInfo) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_type => {
                    self.lexer.next_tok()?;
                    info.type_ = constraint_type_from_token(&self.lexer.from_utf8());
                    self.lexer.need_right()?;
                }

                T::T_uuid => {
                    self.lexer.next_tok()?;
                    info.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }

                T::T_members => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        if self.lexer.next_tok()? != T::T_member {
                            return Err(self.lexer.expecting_str("member"));
                        }

                        // member uuid; resolved (and remapped on append) in resolveConstraints
                        self.lexer.next_tok()?;
                        let member_id = Kiid::from_string_or_random(
                            self.lexer.cur_str(),
                            &mut self.board.kiid_generator,
                        );

                        // anchor token
                        self.lexer.next_tok()?;
                        let anchor = constraint_anchor_from_token(&self.lexer.from_utf8());

                        // Only VERTEX may carry an ordinal and even then optional
                        // writer never emits one elsewhere so accepting one there breaks
                        // round-trip
                        let mut index = -1;

                        if anchor == ConstraintAnchor::Vertex {
                            token = self.lexer.next_tok()?;

                            if token != T::T_RIGHT {
                                if token != T::T_NUMBER {
                                    return Err(self.lexer.expecting_str("vertex index"));
                                }

                                index = self.parse_int();
                                self.lexer.need_right()?;
                            }
                        } else {
                            self.lexer.need_right()?;
                        }

                        info.members
                            .push(ConstraintMember::new(member_id, anchor, index));
                        token = self.lexer.next_tok()?;
                    }
                }

                T::T_value => {
                    self.lexer.next_tok()?;
                    info.value = Some(self.parse_double()?);
                    self.lexer.need_right()?;
                }

                T::T_driving => {
                    info.driving = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_custom_property => {
                    self.parse_custom_property_map(&mut info.custom_properties)?;
                }

                _ => {
                    return Err(self
                        .lexer
                        .expecting_str("type, uuid, members, value, or driving"));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `resolveConstraints( BOARD_ITEM* aParent )`, called by `finishItem` after a board
    /// or footprint is read: creates every recorded constraint (in `m_constraintInfos` order),
    /// resolves its members and adds it to its own recorded parent.
    ///
    /// Mirror resolveGroups: resolve every deferred constraint against the parsed items, adding
    /// each to its own recorded parent (a board parse also sees footprint-scoped constraints,
    /// so the resolution must not assume aParent owns them).
    pub(crate) fn resolve_constraints(&mut self, parent: ParentRef) -> Result<(), IoError> {
        // BOARD* board = dynamic_cast<BOARD*>( aParent );
        // FOOTPRINT* footprint = board ? nullptr : dynamic_cast<FOOTPRINT*>( aParent );
        let is_board = parent == ParentRef::Board;
        let footprint: Option<FootprintId> = match parent {
            ParentRef::Item(id) if id.kind() == ItemKind::Footprint => id.typed(),
            _ => None,
        };

        let mut fp_item_map: HashMap<Kiid, ItemId> = HashMap::new();

        if let Some(fp) = footprint {
            let mut children: Vec<ItemId> = Vec::new();
            self.board
                .item_run_on_children(fp.into(), RecurseMode::NoRecurse, &mut |child| {
                    children.push(child)
                });

            for child in children {
                if let Some(uuid) = self.board.uuid_of(child) {
                    // std::unordered_map::insert: the first item of a UUID wins
                    fp_item_map.entry(uuid).or_insert(child);
                }
            }
        }

        for info in std::mem::take(&mut self.constraint_infos) {
            // std::make_unique<PCB_CONSTRAINT>( info.parent, info.type ): the constructor
            // draws a fresh id that SetUuidDirect then replaces.
            let mut constraint = PcbConstraint::new(&mut self.board.kiid_generator, info.type_);
            constraint.base.parent = info.parent;

            constraint.base.uuid = info.uuid; // SetUuidDirect
            constraint.set_value(info.value);
            constraint.set_driving(info.driving);
            constraint.base.custom_properties = info.custom_properties.clone();

            // constraint->GetParentFootprint(): the recorded parent when it is a footprint.
            let constraint_parent_fp: Option<FootprintId> = match info.parent {
                ParentRef::Item(id) if id.kind() == ItemKind::Footprint => id.typed(),
                _ => None,
            };

            for member in &info.members {
                let mut resolved_id = member.item;

                if self.append_to_existing {
                    // Use the remapped uuid if this member's item was part of the appended
                    // content; otherwise keep the original (it will dangle as an error
                    // state).  find(), not operator[], so a miss does not pollute the remap
                    // with a nil entry.
                    if let Some(&remap) = self.reset_kiid_map.get(&member.item.as_string()) {
                        resolved_id = remap;
                    }
                }

                let item = if is_board {
                    self.board.resolve_item(&resolved_id)
                } else if footprint.is_some() {
                    fp_item_map.get(&resolved_id).copied()
                } else {
                    None
                };

                // A member that resolves to an item in a different footprint scope is
                // genuinely invalid and dropped.  A member that does not resolve at all (its
                // item was deleted) is kept as a dangling reference: the constraint persists
                // in an error state for the user to repair, rather than silently losing it
                // (Zulip "Geometry Constraint Solver", 2026-06-18; supersedes the plan's
                // drop-on-missing rule).
                match item {
                    Some(item) => {
                        let item_parent_fp = {
                            let ctx = BoardCtx::bare(&self.board);
                            ctx.item(item).and_then(|i| i.parent_footprint_id())
                        };

                        if item_parent_fp == constraint_parent_fp {
                            if let Some(uuid) = self.board.uuid_of(item) {
                                constraint.add_member(uuid, member.anchor, member.index);
                            }
                        }
                    }
                    None => {
                        constraint.add_member(resolved_id, member.anchor, member.index);
                    }
                }
            }

            let owned = OwnedItem::Constraint(constraint);

            match (info.parent, constraint_parent_fp) {
                (ParentRef::Item(_), Some(fp)) => {
                    // static_cast<FOOTPRINT*>( info.parent )->Add( constraint, INSERT, true )
                    let _ = self.board.footprint_add(fp, owned, AddMode::Insert);
                }
                _ => {
                    // static_cast<BOARD*>( info.parent )->Add( constraint, INSERT, true )
                    let _ = self.board.add(owned, AddMode::Insert);
                }
            }
        }

        Ok(())
    }
}
