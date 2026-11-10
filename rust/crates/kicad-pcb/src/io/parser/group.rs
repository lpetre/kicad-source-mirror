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
            let uuid =
                Kiid::from_string_or_random(self.lexer.cur_str(), &mut self.board.kiid_generator);
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

#[cfg(test)]
mod tests {
    //! PCBIO-09 tests: the differential oracle (`tests/oracle/pcbio09_golden.sh`) and the ports
    //! of `qa/tests/pcbnew/test_group_load_save.cpp` and `group_saveload.cpp`.

    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
    use std::path::Path;

    use kicad_core::Kiid;

    use crate::board::{AddMode, Board};
    use crate::ids::{GroupId, ItemId, ItemKind};
    use crate::io::{FormatOptions, ParseOptions, format_board, parse_board, parse_board_report};
    use crate::item_ref::OwnedItem;
    use crate::pcb_group::PcbGroup;
    use crate::pcb_text::PcbText;

    const PENDING: &str = "kicad-pcb: port pending: ";

    /// Runs `f`; a panic of a `port_pending!` model stub becomes `Err( stub name )`.
    fn model<R>(f: impl FnOnce() -> R) -> Result<R, String> {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(r) => Ok(r),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned());

                match msg {
                    Some(m) if m.starts_with(PENDING) => Err(m[PENDING.len()..].to_string()),
                    _ => resume_unwind(payload),
                }
            }
        }
    }

    fn staging_parse() -> ParseOptions {
        ParseOptions {
            allow_pending: true,
        }
    }

    fn staging() -> FormatOptions {
        FormatOptions {
            allow_pending: true,
            ..FormatOptions::default()
        }
    }

    fn crate_dir() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn fnv1a64(bytes: &[u8]) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;

        for &c in bytes {
            h ^= u64::from(c);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }

        format!("{h:016x}")
    }

    /// One `(group ..)` block of a written file, as `pcbio09_golden.sh` extracts it.
    struct Block {
        uuid: String,
        depth: usize,
        members: usize,
        header: String,
        text: String,
    }

    /// The `(group ..)` blocks of a prettified file (mirror of the golden script).
    fn group_blocks(text: &str) -> Vec<Block> {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let mut out = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let depth = line.len() - line.trim_start_matches('\t').len();

            if depth == 0
                || !line[depth..].starts_with("(group ")
                || line.trim_end_matches('\n').ends_with(')')
            {
                i += 1;
                continue;
            }

            let close = format!("{})\n", "\t".repeat(depth));
            let mut j = i;

            while j < lines.len() && lines[j] != close {
                j += 1;
            }

            let block = &lines[i..(j + 1).min(lines.len())];
            let header: String = block
                .iter()
                .take_while(|l| !l.contains("(members"))
                .copied()
                .collect();
            let text: String = block.concat();
            let uuid = block
                .iter()
                .find_map(|l| {
                    let t = l.trim_start_matches('\t');
                    t.strip_prefix("(uuid \"")
                        .and_then(|r| r.split('"').next())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let members = text
                .split("(members")
                .nth(1)
                .map(|r| r.split(')').next().unwrap_or("").matches('"').count() / 2)
                .unwrap_or(0);

            out.push(Block {
                uuid,
                depth,
                members,
                header,
                text,
            });
            i = j + 1;
        }

        out
    }

    fn groups_text(text: &str) -> String {
        group_blocks(text)
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .concat()
    }

    /// Parse + format with the staging switches (setup and the other pending sections are
    /// skipped on both sides).
    fn resave(text: &str) -> String {
        let board = parse_board(text, "", &staging_parse()).unwrap();
        format_board(&board, &staging()).unwrap()
    }

    /// Byte level: the `(group ..)` blocks written for the hand-made boards equal those of the
    /// reference `kicad-cli pcb upgrade --force`, and so do those of the reference output re-saved.
    #[test]
    fn oracle_resave_groups() {
        for name in ["groups.kicad_pcb", "groups_v20231212.kicad_pcb"] {
            let input =
                std::fs::read_to_string(crate_dir().join("tests/data/pcbio09").join(name)).unwrap();
            let golden =
                std::fs::read_to_string(crate_dir().join("tests/golden/pcbio09").join(name))
                    .unwrap();
            let expected = groups_text(&golden);
            assert!(!expected.is_empty(), "{name}");

            assert_eq!(groups_text(&resave(&input)), expected, "{name}: input");

            // The reference re-save of the reference output (`_o2` when it differs: a group
            // whose members were only empty groups is not written the second time).
            let o2_path = crate_dir()
                .join("tests/golden/pcbio09")
                .join(name.replace(".kicad_pcb", "_o2.kicad_pcb"));
            let expected2 = match std::fs::read_to_string(o2_path) {
                Ok(o2) => groups_text(&o2),
                Err(_) => expected,
            };
            assert_eq!(groups_text(&resave(&golden)), expected2, "{name}: golden");
        }
    }

    /// Item level: `pcb-dump board --no-init --sections misc,item` of the hand-made boards
    /// (`tests/golden/pcbio09/*.dump.txt`) against the Rust dump.
    #[cfg(feature = "dump")]
    #[test]
    fn oracle_dump_groups() {
        use crate::dump::{DumpOptions, compare::compare, dump_board, scan_file_uuids};

        for name in ["groups", "groups_v20231212"] {
            let path = crate_dir()
                .join("tests/data/pcbio09")
                .join(format!("{name}.kicad_pcb"));
            let text = std::fs::read_to_string(&path).unwrap();
            let golden = std::fs::read_to_string(
                crate_dir()
                    .join("tests/golden/pcbio09")
                    .join(format!("{name}.dump.txt")),
            )
            .unwrap();
            let board = parse_board(&text, "", &staging_parse()).unwrap();
            let opts = DumpOptions {
                sections: ["misc", "item"].iter().map(|s| s.to_string()).collect(),
                hash: false,
                file_uuids: Some(scan_file_uuids(text.as_bytes())),
            };
            let rust = format!("file|{name}.kicad_pcb|\n{}", dump_board(&board, &opts));
            let cmp = compare(&golden, &rust);

            // `locked_flag` is the C++ non-virtual `BOARD_ITEM::IsLocked()` (parent group
            // included); the Rust dump prints the raw flag (change request to DUMP-RS). The
            // virtual `locked` field checks the same thing.
            let failures: Vec<_> = cmp
                .lines
                .iter()
                .filter(|l| l.0.is_failure() && !l.1.ends_with("|locked_flag"))
                .collect();

            assert!(failures.is_empty(), "{name}:\n{}", cmp.report(20));
        }
    }

    /// The error texts of the reference build for a bad design block link and an unknown
    /// token (`pcb-dump` load errors of `tests/data/pcbio09/groups_bad*.kicad_pcb`).
    #[test]
    fn parse_errors() {
        let cases = [
            (
                "groups_badlib.kicad_pcb",
                "Group library link mylib:x:y contains invalid character 'y' in '', line 5, \
                 offset 69.",
            ),
            (
                "groups_badtok.kicad_pcb",
                "Expecting uuid, locked, lib_id, or members. Got ''layer'' in '', line 5, \
                 offset 61.",
            ),
        ];

        for (name, expected) in cases {
            let text =
                std::fs::read_to_string(crate_dir().join("tests/data/pcbio09").join(name)).unwrap();
            let e = parse_board(&text, "", &staging_parse()).err().unwrap();
            assert_eq!(e.what(), expected, "{name}");
        }

        let e = parse_board(
            "(kicad_pcb (version 20240101) (group \"g\" foo (uuid \"x\")))",
            "",
            &staging_parse(),
        )
        .err()
        .unwrap();
        assert_eq!(
            e.what(),
            "Expecting group name or locked. Got ''symbol'' in '', line 1, offset 42."
        );
    }

    /// Corpus, byte level: every `(group ..)` block of the corpus boards re-saved by the
    /// reference build (`tests/golden/pcbio09_corpus_groups.txt`). A block is compared in full
    /// when the parse skipped nothing; otherwise its header (name, uuid, lock, lib_id) must be
    /// identical and it may list fewer members (those whose item parser is still a staging
    /// stub); a group whose members are all pending is not written and counts as pending.
    #[test]
    fn oracle_corpus_groups() {
        let golden =
            std::fs::read_to_string(crate_dir().join("tests/golden/pcbio09_corpus_groups.txt"))
                .unwrap();
        let root = kicad_qa::kicad_source_root();
        let mut boards: Vec<(&str, Vec<Vec<&str>>)> = Vec::new();

        for line in golden
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let fields: Vec<&str> = line.split('|').collect();

            match boards.last_mut() {
                Some((path, rows)) if *path == fields[0] => rows.push(fields[1..].to_vec()),
                _ => boards.push((fields[0], vec![fields[1..].to_vec()])),
            }
        }

        let (mut full, mut header_only, mut pending, mut stub_boards) = (0, 0, 0, 0);
        let mut failures = Vec::new();

        for (path, rows) in &boards {
            if rows[0][0] == "error" || rows[0][0] == "none" {
                continue;
            }

            let file = root.join(path);

            // The large demo boards are the pcbio01 `corpus_large` set (ignored there too).
            if std::fs::metadata(&file).unwrap().len() >= 20_000_000 {
                pending += rows.len();
                continue;
            }

            let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
            let result = model(|| {
                let (board, report) = parse_board_report(&text, "", &staging_parse())
                    .map_err(|e| e.what().to_string())?;
                let out = format_board(&board, &staging()).map_err(|e| e.what().to_string())?;
                Ok::<_, String>((out, report.skipped.is_empty()))
            });

            let (out, complete) = match result {
                Err(stub) => {
                    stub_boards += 1;
                    pending += rows.len();
                    println!("{path}: pending on model stub {stub}");
                    continue;
                }
                Ok(Err(e)) => {
                    failures.push(format!("{path}: {e}"));
                    continue;
                }
                Ok(Ok(r)) => r,
            };

            let blocks = group_blocks(&out);

            for row in rows {
                let (uuid, depth, members, header, block) = (
                    row[0],
                    row[1].parse::<usize>().unwrap(),
                    row[2]
                        .trim_start_matches("members=")
                        .parse::<usize>()
                        .unwrap(),
                    row[3],
                    row[4],
                );

                let Some(b) = blocks.iter().find(|b| b.uuid == uuid && b.depth == depth) else {
                    if complete {
                        failures.push(format!("{path}: group {uuid} not written"));
                    } else {
                        pending += 1;
                    }
                    continue;
                };

                if fnv1a64(b.text.as_bytes()) == block {
                    full += 1;
                } else if complete {
                    failures.push(format!("{path}: group {uuid} differs:\n{}", b.text));
                } else if fnv1a64(b.header.as_bytes()) != header {
                    failures.push(format!(
                        "{path}: group {uuid} header differs:\n{}",
                        b.header
                    ));
                } else if b.members > members {
                    failures.push(format!(
                        "{path}: group {uuid}: {} members > {members}",
                        b.members
                    ));
                } else {
                    header_only += 1;
                }
            }
        }

        println!(
            "pcbio09 corpus groups: {full} identical, {header_only} identical header with \
             pending members, {pending} pending ({stub_boards} boards on model stubs)"
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // ---- qa/tests/pcbnew/test_group_load_save.cpp -------------------------------------------

    /// `GroupsLoadSave`: the group of `groups_load_save*.kicad_pcb` (locked, name, member
    /// count). Its two members are graphic items (PCBIO-03 / PCBIO-05): while a parser of the
    /// file is a staging stub, the member count is only bounded.
    #[test]
    fn groups_load_save() {
        for (file, version) in [
            ("groups_load_save.kicad_pcb", 20231231),
            ("groups_load_save_v20231212.kicad_pcb", 20231212),
        ] {
            let text = std::fs::read_to_string(kicad_qa::pcbnew_data_dir().join(file)).unwrap();
            let parsed = model(|| parse_board_report(&text, "", &staging_parse()).unwrap());
            let Ok((board, report)) = parsed else {
                println!(
                    "groups_load_save: {file}: pending on model stub {:?}",
                    parsed.err()
                );
                continue;
            };
            assert_eq!(board.file_format_version_at_load, version, "{file}");

            let uuid = Kiid::parse("a78cc65c-451e-451e-9147-4460cc669685").unwrap();
            let id = board.resolve_item(&uuid).expect("group");
            assert_eq!(id.kind(), ItemKind::Group);

            let ctx = crate::ctx::BoardCtx::bare(&board);
            let item = ctx.item(id).unwrap();
            let group = item.as_group().unwrap();
            assert!(item.is_locked(), "{file}");
            assert_eq!(group.name(), "GroupName");

            if report.skipped.is_empty() {
                assert_eq!(group.items().len(), 2, "{file}");
            } else {
                // Members whose parser is still a staging stub are not on the board.
                assert!(group.items().len() <= 2, "{file}");
                println!(
                    "groups_load_save: {file}: {} of 2 members parsed (pending: {:?})",
                    group.items().len(),
                    report.skipped.iter().map(|s| &s.token).collect::<Vec<_>>()
                );
            }
        }
    }

    // ---- qa/tests/pcbnew/group_saveload.cpp -------------------------------------------------

    const TEXT0: usize = 0;
    const TEXT1: usize = 1;
    const TEXT2: usize = 2;
    const TEXT3: usize = 3;
    const TEXT4: usize = 4;
    const TEXT5: usize = 5;
    const TEXT6: usize = 6;
    const REMOVED_TEXT: usize = 9;
    const GROUP0: usize = 10;
    const GROUP1: usize = 11;
    const NAME_GROUP3: usize = 13;
    const NUM_ITEMS: usize = 17;
    const REMOVED_GROUP: usize = 16;

    /// `createBoard( spec )` (valid groups only): texts 0..8 on the board, groups added when
    /// they have a spec or are used by another group.
    fn create_board(spec: &[&[usize]]) -> Board {
        let mut board = Board::new();
        let mut items: Vec<Option<ItemId>> = Vec::new();

        for idx in 0..=REMOVED_TEXT {
            let mut text = PcbText::new(&mut board.kiid_generator);
            text.text.set_text(&format!("some text-{idx}"));

            // Don't add REMOVED_TEXT to the board
            if idx < REMOVED_TEXT {
                items.push(Some(
                    board
                        .add(OwnedItem::Text(Box::new(text)), AddMode::Insert)
                        .unwrap(),
                ));
            } else {
                items.push(None);
            }
        }

        let mut groups: Vec<PcbGroup> = Vec::new();

        for idx in 0..(NUM_ITEMS - GROUP0) {
            let mut gr = PcbGroup::new(&mut board.kiid_generator);

            if idx >= NAME_GROUP3 - GROUP0 {
                let n = if idx == 15 - GROUP0 { 3 } else { idx };
                gr.set_name(&format!("group-{n}"));
            }

            groups.push(gr);
        }

        // The groups that end up on the board: those with a spec or used by another one.
        let mut used = [false; NUM_ITEMS];

        for group_spec in spec {
            for &item in *group_spec {
                used[item] = true;
            }
        }

        for (offset, gr) in groups.into_iter().enumerate() {
            let group_idx = GROUP0 + offset;

            if (offset < spec.len() || used[group_idx]) && group_idx != REMOVED_GROUP {
                let id: GroupId = board.add_group(gr, AddMode::Insert);
                items.push(Some(id.into()));
            } else {
                items.push(None);
            }
        }

        for (offset, group_spec) in spec.iter().enumerate() {
            let group = items[GROUP0 + offset].unwrap();

            for &item in *group_spec {
                board.add_to_group(group, items[item].unwrap());
            }

            assert_eq!(board.group_members(group).count(), group_spec.len());
        }

        board
    }

    /// `testGroupsEqual`: same groups (by uuid), names and member uuids.
    fn group_signature(board: &Board) -> Vec<(Kiid, String, Vec<Kiid>)> {
        let mut out: Vec<_> = board
            .groups()
            .map(|(id, g)| {
                let mut members: Vec<Kiid> = board
                    .group_members(id.into())
                    .filter_map(|m| board.uuid_of(m))
                    .collect();
                members.sort();
                (g.base.uuid, g.name().to_string(), members)
            })
            .collect();
        out.sort_by_key(|g| g.0);
        out
    }

    /// `testSaveLoad`: dump to a file and read it back. While the text writer (PCBIO-05) is a
    /// staging stub the round trip is done with the staging switches and only the group to
    /// group memberships can survive it: those are checked, and the test says it is pending.
    fn test_save_load(spec: &[&[usize]]) {
        let board1 = create_board(spec);
        let expected = group_signature(&board1);

        match model(|| format_board(&board1, &FormatOptions::default())) {
            Ok(Ok(text)) => match model(|| parse_board(&text, "", &staging_parse()).unwrap()) {
                Ok(board2) => assert_eq!(group_signature(&board2), expected),
                Err(stub) => println!("group_saveload: pending on model stub {stub}"),
            },
            Ok(Err(e)) if e.what().starts_with("port pending: ") => {
                println!(
                    "group_saveload: {} - checking group-to-group membership",
                    e.what()
                );
                let text = format_board(&board1, &staging()).unwrap();
                let board2 = parse_board(&text, "", &staging_parse()).unwrap();
                let groups: Vec<Kiid> = expected.iter().map(|g| g.0).collect();

                // A group whose members are all texts is not written: it is lost.
                let surviving: Vec<_> = expected
                    .iter()
                    .filter(|g| g.2.iter().any(|m| groups.contains(m)))
                    .map(|(uuid, name, members)| {
                        let members: Vec<Kiid> = members
                            .iter()
                            .filter(|m| groups.contains(m))
                            .copied()
                            .collect();
                        (*uuid, name.clone(), members)
                    })
                    .collect();
                let got: Vec<_> = group_signature(&board2)
                    .into_iter()
                    .filter(|g| !g.2.is_empty())
                    .collect();
                assert_eq!(got, surviving);
            }
            Ok(Err(e)) => panic!("{}", e.what()),
            Err(stub) => println!("group_saveload: pending on model stub {stub}"),
        }
    }

    /// `HealthyGroups`.
    #[test]
    fn healthy_groups() {
        // Test board with no groups
        test_save_load(&[]);

        // Single group with 2 members
        test_save_load(&[&[TEXT0, TEXT1]]);

        // Two groups
        test_save_load(&[&[TEXT0, TEXT1], &[TEXT2, TEXT3]]);
        test_save_load(&[&[TEXT0, TEXT1], &[TEXT2, GROUP0]]);

        // Subgroups with 2+ members each
        test_save_load(&[&[TEXT0, TEXT1, GROUP1], &[TEXT2, TEXT3], &[TEXT4, GROUP0]]);
        test_save_load(&[&[TEXT0, TEXT1], &[TEXT2, TEXT3], &[GROUP1, GROUP0]]);
        test_save_load(&[
            &[TEXT0, TEXT1],
            &[TEXT2, TEXT3],
            &[TEXT4, NAME_GROUP3],
            &[TEXT5, TEXT6],
        ]);
    }

    /// `SingleMemberGroupsSaved`.
    #[test]
    fn single_member_groups_saved() {
        let board1 = create_board(&[&[TEXT0]]);

        match model(|| format_board(&board1, &FormatOptions::default())) {
            Ok(Ok(text)) => match model(|| parse_board(&text, "", &staging_parse()).unwrap()) {
                Ok(board2) => assert_eq!(board2.groups().count(), 1),
                Err(stub) => println!("single_member_groups_saved: pending on model stub {stub}"),
            },
            Ok(Err(e)) if e.what().starts_with("port pending: ") => {
                // The text is not written yet, so the group is empty and not written either.
                println!("single_member_groups_saved: pending ({})", e.what());
                let text = format_board(&board1, &staging()).unwrap();
                assert!(!text.contains("(group "));
            }
            Ok(Err(e)) => panic!("{}", e.what()),
            Err(stub) => println!("single_member_groups_saved: pending on model stub {stub}"),
        }
    }

    /// The group sanity check after loading: a cycle in the file cannot be built
    /// (`EDA_GROUP::AddItem` refuses an ancestor), so `resolveGroups` leaves a sane board
    /// (`InvalidGroups` itself is the M22 test of `GroupsSanityCheck`).
    #[test]
    fn cycles_in_file_are_refused() {
        let text = std::fs::read_to_string(
            crate_dir().join("tests/data/pcbio09/groups_v20231212.kicad_pcb"),
        )
        .unwrap();
        let mut board = parse_board(&text, "", &staging_parse()).unwrap();
        assert_eq!(board.groups_sanity_check(false), "");
        let d = board
            .resolve_item(&Kiid::parse("d78cc65c-451e-451e-9147-4460cc669685").unwrap())
            .unwrap();
        assert_eq!(board.group_members(d).count(), 0);
    }
}
