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

//! Port of the group part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-09): `parseGROUP_members`, `parseGROUP`, `resolveGroups`.
//!
//! Groups (and generators, which are groups) are only *recorded* while the file is read
//! ([`GroupInfo`] / [`GeneratorInfo`]); `finishItem` calls [`PcbParser::resolve_groups`] once
//! the board or library footprint is complete, which creates the group objects, resolves the
//! member UUIDs and runs the group sanity check in repair mode.
//!
//! Generators reuse this machinery: `parseGENERATOR` (PCBIO-13) pushes a [`GeneratorInfo`]
//! (whose `group` part is filled with [`PcbParser::parse_group_members`] and the same fields as
//! a group), and [`PcbParser::resolve_groups`] turns it into a `PcbGenerator` through
//! [`PcbParser::generator_from_info`], the generator-specific part of pass 1.

use std::collections::HashMap;

use kicad_core::{KicadT, Kiid};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use super::{GeneratorInfo, GroupInfo};
use crate::board::AddMode;
use crate::ctx::BoardCtx;
use crate::ids::{FootprintId, ItemId, ItemKind};
use crate::item::{ParentRef, RecurseMode};
use crate::item_ref::{AnyItem, OwnedItem};
use crate::pcb_generator::{GENERATOR_TYPE_VIA_STITCH, PcbGenerator};
use crate::pcb_group::PcbGroup;
use crate::pcb_track::Track;
use crate::traits::BoardItem;

/// One entry of the C++ `std::vector<GROUP_INFO*> groupTypeObjects`: a plain group or a
/// generator declaration (`dynamic_cast<GENERATOR_INFO*>`).
enum GroupTypeObject {
    Group(GroupInfo),
    Generator(GeneratorInfo),
}

impl GroupTypeObject {
    /// The `GROUP_INFO` part.
    fn info(&self) -> &GroupInfo {
        match self {
            GroupTypeObject::Group(info) => info,
            GroupTypeObject::Generator(info) => &info.group,
        }
    }
}

impl PcbParser {
    /// Port of `parseGROUP_members( GROUP_INFO& )`: every token up to the closing `)` is the
    /// UUID of a member, kept in file order.
    pub(crate) fn parse_group_members(
        &mut self,
        group_info: &mut GroupInfo,
    ) -> Result<(), IoError> {
        while self.lexer.next_tok()? != T::T_RIGHT {
            // This token is the Uuid of the item in the group.
            // Since groups are serialized at the end of the file/footprint, the Uuid should
            // already have been seen and exist in the board.
            //
            // KIID( CurStr() ): a string that is neither a UUID nor a legacy time stamp draws
            // a random id.
            let uuid = Kiid::from_string_or_random(
                &self.lexer.cur_str().to_string(),
                &mut self.board.kiid_generator,
            );
            group_info.member_uuids.push(uuid);
        }

        Ok(())
    }

    /// Port of `parseGROUP( BOARD_ITEM* aParent )`: records a [`GroupInfo`] in
    /// `m_groupInfos`; the group itself is created by [`resolve_groups`](Self::resolve_groups).
    pub(crate) fn parse_group(&mut self, parent: ParentRef) -> Result<(), IoError> {
        // wxCHECK_RET( CurTok() == T_group, ... )
        if self.lexer.cur_tok() != T::T_group {
            debug_assert!(
                false,
                "Cannot parse {} as PCB_GROUP.",
                self.lexer.cur_text()
            );
            return Ok(());
        }

        // m_groupInfos.push_back( GROUP_INFO() ): the value-initialised KIID member draws a
        // fresh id (KIID's default constructor), used when the file has no (uuid ...).
        let mut group_info = GroupInfo {
            parent,
            uuid: Kiid::new(&mut self.board.kiid_generator),
            ..GroupInfo::default()
        };

        let result = self.parse_group_body(&mut group_info);

        // The C++ fills the vector entry in place: a declaration interrupted by a parse error
        // stays recorded (harmless, the parse fails).
        self.group_infos.push(group_info);

        result
    }

    /// The token loops of `parseGROUP` (after the `GROUP_INFO` was pushed).
    fn parse_group_body(&mut self, group_info: &mut GroupInfo) -> Result<(), IoError> {
        let mut token;

        loop {
            token = self.lexer.next_tok()?;

            if token == T::T_LEFT {
                break;
            }

            if token == T::T_STRING {
                group_info.name = self.lexer.from_utf8();
            } else if token == T::T_locked {
                group_info.locked = true;
            } else {
                return Err(self.lexer.expecting_str("group name or locked"));
            }
        }

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                // From formats [20200811, 20231215), 'id' was used instead of 'uuid'
                T::T_id | T::T_uuid => {
                    self.lexer.next_tok()?;
                    group_info.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }

                T::T_lib_id => {
                    token = self.lexer.next_tok()?;

                    if !kicad_sexpr::DsnLexer::<T>::is_symbol(token) && token != T::T_NUMBER {
                        return Err(self.lexer.expecting_str("symbol|number"));
                    }

                    // Some symbol LIB_IDs have the '/' character escaped which can break
                    // symbol links.  The '/' character is no longer an illegal LIB_ID character
                    // so it doesn't need to be escaped.
                    let name = self.lexer.from_utf8().replace("{slash}", "/");

                    let bad_pos = group_info.lib_id.parse(&name, false);

                    if bad_pos >= 0 {
                        // `static_cast<int>( name.size() ) > bad_pos` and `name[bad_pos]` index
                        // the wxString by character, while LIB_ID::Parse returns a byte offset
                        // into the item name part: reproduced as is.
                        let bad_pos = bad_pos as usize;

                        if let Some(bad_char) = name.chars().nth(bad_pos) {
                            let msg = format!(
                                "Group library link {name} contains invalid character '{bad_char}'"
                            );

                            return Err(self.lexer.parse_error(msg));
                        }

                        return Err(self.lexer.parse_error("Invalid library ID"));
                    }

                    self.lexer.need_right()?;
                }

                T::T_locked => {
                    group_info.locked = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_members => {
                    self.parse_group_members(group_info)?;
                }

                T::T_custom_property => {
                    self.parse_custom_property_map(&mut group_info.custom_properties)?;
                }

                _ => {
                    return Err(self.lexer.expecting_str("uuid, locked, lib_id, or members"));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `resolveGroups( BOARD_ITEM* aParent )`, called by `finishItem` after a board or
    /// footprint is read: creates the recorded groups and generators (in `m_groupInfos` then
    /// `m_generatorInfos` order), resolves their member UUIDs and repairs group cycles.
    ///
    /// The declarations are consumed (C++ moves the generator templates out and `finishItem`
    /// clears the lists right after).
    pub(crate) fn resolve_groups(&mut self, parent: ParentRef) -> Result<(), IoError> {
        // BOARD* board = dynamic_cast<BOARD*>( aParent );
        // FOOTPRINT* footprint = board ? nullptr : dynamic_cast<FOOTPRINT*>( aParent );
        let is_board = parent == ParentRef::Board;
        let footprint: Option<FootprintId> = match parent {
            ParentRef::Item(id) if id.kind() == ItemKind::Footprint => id.typed(),
            _ => None,
        };

        // For footprint parents, build a one-time lookup map instead of scanning children
        // on every call.  For board parents, resolve using BOARD::ResolveItem(), which
        // goes to the board's item index cache and re-binds stale entries.
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

        // Now that we've parsed the other Uuids in the file we can resolve the uuids referred
        // to in the group declarations we saw.
        //
        // First add all group objects so subsequent getItem() calls for nested groups work.
        let mut group_type_objects: Vec<GroupTypeObject> = Vec::new();

        for group_info in std::mem::take(&mut self.group_infos) {
            group_type_objects.push(GroupTypeObject::Group(group_info));
        }

        for gen_info in std::mem::take(&mut self.generator_infos) {
            group_type_objects.push(GroupTypeObject::Generator(gen_info));
        }

        let mut infos: Vec<GroupInfo> = Vec::with_capacity(group_type_objects.len());

        for object in group_type_objects {
            let item = match object {
                GroupTypeObject::Generator(mut gen_info) => {
                    let mut generator = self.generator_from_info(&mut gen_info)?;
                    let group_info = gen_info.group;

                    generator.group.base.uuid = group_info.uuid; // SetUuidDirect
                    generator.group.base.custom_properties = group_info.custom_properties.clone();

                    if group_info.lib_id.is_valid() {
                        generator
                            .group
                            .set_design_block_lib_id(group_info.lib_id.clone());
                    }

                    if group_info.locked {
                        generator.group.base.is_locked = true;
                    }

                    infos.push(group_info);
                    OwnedItem::Generator(Box::new(generator))
                }
                GroupTypeObject::Group(group_info) => {
                    // new PCB_GROUP( groupInfo->parent ): the constructor draws a fresh id that
                    // SetUuidDirect then replaces.
                    let mut group = PcbGroup::new(&mut self.board.kiid_generator);
                    group.base.parent = group_info.parent;
                    group.set_name(&group_info.name);

                    group.base.uuid = group_info.uuid; // SetUuidDirect
                    group.base.custom_properties = group_info.custom_properties.clone();

                    if group_info.lib_id.is_valid() {
                        group.set_design_block_lib_id(group_info.lib_id.clone());
                    }

                    if group_info.locked {
                        group.base.is_locked = true; // PCB_GROUP::SetLocked( true )
                    }

                    infos.push(group_info);
                    OwnedItem::Group(group)
                }
            };

            let group_parent = infos.last().map_or(ParentRef::None, |info| info.parent);

            match group_parent {
                ParentRef::Item(owner) if owner.kind() == ItemKind::Footprint => {
                    let added = match owner.typed() {
                        Some(fp) => self.board.footprint_add(fp, item, AddMode::Insert).ok(),
                        None => None,
                    };

                    // A generator is not accepted by FOOTPRINT::Add (no such case; the C++
                    // leaks it): it simply does not exist afterwards.
                    if let Some(id) = added {
                        self.after_generator_add(id);

                        // Keep the footprint lookup map in sync with newly added groups
                        if footprint.is_some() {
                            if let Some(uuid) = self.board.uuid_of(id) {
                                fp_item_map.entry(uuid).or_insert(id);
                            }
                        }
                    }
                }
                _ => {
                    // static_cast<BOARD*>( groupInfo->parent )->Add( group, ADD_MODE::INSERT,
                    // true )
                    if let Ok(id) = self.board.add(item, AddMode::Insert) {
                        self.after_generator_add(id);
                    }
                }
            }
        }

        let get_item = |board: &crate::board::Board,
                        map: &HashMap<Kiid, ItemId>,
                        id: &Kiid|
         -> Option<ItemId> {
            if is_board {
                board.resolve_item(id)
            } else if footprint.is_some() {
                map.get(id).copied()
            } else {
                None
            }
        };

        for group_info in &infos {
            let Some(group) = get_item(&self.board, &fp_item_map, &group_info.uuid) else {
                continue;
            };

            // dynamic_cast<PCB_GROUP*>: a group or a generator
            if !matches!(group.kind(), ItemKind::Group | ItemKind::Generator) {
                continue;
            }

            let group_parent_fp = self.parent_footprint_of(group);

            for uuid in &group_info.member_uuids {
                let item = if self.append_to_existing {
                    // m_resetKIIDMap[ aUuid.AsString() ] (operator[] inserts a fresh KIID)
                    let key = uuid.as_string();
                    let mapped = match self.reset_kiid_map.get(&key) {
                        Some(&mapped) => mapped,
                        None => {
                            let fresh = Kiid::new(&mut self.board.kiid_generator);
                            self.reset_kiid_map.insert(key, fresh);
                            fresh
                        }
                    };
                    get_item(&self.board, &fp_item_map, &mapped)
                } else {
                    get_item(&self.board, &fp_item_map, uuid)
                };

                // We used to allow fp items in non-footprint groups.  It was a mistake.  Check
                // to make sure they the item and group are owned by the same parent (will both
                // be nullptr in the board case).
                if let Some(item) = item {
                    if self.parent_footprint_of(item) == group_parent_fp {
                        self.board.add_to_group(group, item);
                    }
                }
            }

            // For generators, set the layer to match the layer of the contained tracks.
            // GetBoardItems() is unordered, so this may only be done for a generator whose
            // members all share one layer; one that spans layers keeps its own.
            if group.kind() == ItemKind::Generator {
                let layer = {
                    let ctx = BoardCtx::bare(&self.board);
                    let follows = ctx.item(group).is_some_and(|g| match g.any() {
                        AnyItem::Generator(generator) => generator.layer_follows_members(),
                        _ => false,
                    });

                    if follows {
                        // gen->GetBoardItems(): the members (insertion order, see
                        // PORT_STATUS.md), first PCB_TRACK (trace, arc or via)
                        self.board.group_members(group).find_map(|member| {
                            let member = ctx.item(member)?;
                            matches!(
                                member.type_(),
                                KicadT::PcbTraceT | KicadT::PcbArcT | KicadT::PcbViaT
                            )
                            .then(|| member.layer())
                        })
                    } else {
                        None
                    }
                };

                if let Some(layer) = layer {
                    // PCB_GENERATOR::SetLayer: m_layer = aLayer
                    self.board.modify(group, |mut item, _| {
                        item.as_dyn_mut().base_mut().layer = layer;
                    });
                }
            }
        }

        // Don't allow group cycles
        if self.board_present {
            self.board.groups_sanity_check(true);
        }

        Ok(())
    }

    /// The generator-specific part of pass 1 of `resolveGroups` (the
    /// `dynamic_cast<GENERATOR_INFO*>` branch): `GENERATORS_MGR::CreateFromType`, `SetLayer`,
    /// `SetProperties`, `SetTemplateItem`. The common group fields (UUID, custom properties,
    /// design block link, lock) are set by the caller.
    ///
    /// Hook for PCBIO-13 (`parseGENERATOR`), which fills `m_generatorInfos`.
    ///
    /// # Errors
    ///
    /// `IO_ERROR` "Cannot create generated object of type '%s'" for an unknown type.
    pub(crate) fn generator_from_info(
        &mut self,
        gen_info: &mut GeneratorInfo,
    ) -> Result<PcbGenerator, IoError> {
        let Some(mut generator) = PcbGenerator::create_from_type(&gen_info.gen_type) else {
            return Err(IoError::new(format!(
                "Cannot create generated object of type '{}'",
                gen_info.gen_type
            )));
        };

        // `new T` in GENERATORS_MGR::REGISTER: the EDA_ITEM constructor draws a fresh id that
        // SetUuidDirect later replaces.
        let _ = Kiid::new(&mut self.board.kiid_generator);

        generator.group.base.parent = gen_info.group.parent;

        // gen->SetLayer( genInfo->layer )
        generator.group.base.layer = gen_info.layer;

        // gen->SetProperties( genInfo->properties ): the subclass reads the map into its typed
        // members; the port keeps the map and normalises it the way the typed members would.
        generator.properties = gen_info.properties.clone();
        generator.normalize_properties();

        for (name, item) in std::mem::take(&mut gen_info.templates) {
            // gen->SetTemplateItem( name, std::move( item ) ): a no-op in PCB_GENERATOR;
            // PCB_VIA_STITCH keeps a "via" template (one, the last wins) and marks it free.
            if generator.generator_type != GENERATOR_TYPE_VIA_STITCH || name != "via" {
                continue;
            }

            let OwnedItem::Track(Track::Via(mut via)) = item else {
                continue;
            };

            via.is_free = true; // SetIsFree( true )
            generator.templates.retain(|(n, _)| n != "via");
            generator
                .templates
                .push((name, OwnedItem::Track(Track::Via(via))));
        }

        Ok(generator)
    }

    /// The `SetParent( this )` of `PCB_VIA_STITCH::SetTemplateItem`, done once the generator
    /// has its handle. Nothing to do for a group.
    fn after_generator_add(&mut self, id: ItemId) {
        if id.kind() != ItemKind::Generator {
            return;
        }

        if let Some(generator) = id
            .typed()
            .and_then(|gen_id| self.board.generators.get_mut(gen_id))
        {
            for (_, template) in &mut generator.templates {
                if let OwnedItem::Track(track) = template {
                    track.base_mut().parent = ParentRef::Item(id);
                }
            }
        }
    }

    /// `GetParentFootprint()` of a stored item.
    fn parent_footprint_of(&self, id: ItemId) -> Option<FootprintId> {
        let ctx = BoardCtx::bare(&self.board);
        ctx.item(id).and_then(|item| item.parent_footprint_id())
    }
}
