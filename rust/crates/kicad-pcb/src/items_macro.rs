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

//! The `board_items!` table: the single list of arena item kinds (new code, `board-model.md`
//! section 2).
//!
//! `board_items!(callback)` invokes the generator macro `callback!` with one row per arena
//! kind; `board_items!(callback, { tokens })` passes `tokens` first. The generators below
//! expand the rows into:
//!
//! * `ids.rs`: `ItemKind` (with the non-arena `Net` kind appended), the typed handle aliases
//!   (`PadId = Id<Pad>`, ...) and the `ArenaItem` impls (`gen_ids!`);
//! * `item_ref.rs`: `AnyItem`, `AnyItemMut` and their dispatch helpers (`gen_any_item!`);
//! * `board/mod.rs`: the arena fields of `Board`, `Index<XxxId> for Board`, the typed getters,
//!   `Board::get` / `contains`, the per-kind `modify_*` shells and the per-arena part of
//!   `check_invariants` (`gen_board!`).
//!
//! Adding an item type is one row here plus its struct (and its trait shims).
//!
//! Row format: `Kind = discriminant, HandleAlias, arena_field, RustType, getter, modify_fn;`.
//! Discriminants are part of the wasm ABI (`ItemId::to_bits`): append only.

/// The table of arena item kinds; see the module documentation.
macro_rules! board_items {
    ($cb:ident) => {
        board_items! { $cb, {} }
    };
    ($cb:ident, { $($extra:tt)* }) => {
        $cb! {
            { $($extra)* }
            Footprint = 1, FootprintId, footprints, crate::footprint::Footprint, footprint, modify_footprint;
            Pad = 2, PadId, pads, crate::pad::Pad, pad, modify_pad;
            Track = 3, TrackId, tracks, crate::pcb_track::Track, track, modify_track;
            Zone = 4, ZoneId, zones, crate::zone::Zone, zone, modify_zone;
            Shape = 5, ShapeId, shapes, crate::pcb_shape::PcbShape, shape, modify_shape;
            Text = 6, TextId, texts, crate::pcb_text::PcbText, text, modify_text;
            Field = 7, FieldId, fields, crate::pcb_field::PcbField, field, modify_field;
            Textbox = 8, TextboxId, textboxes, crate::pcb_textbox::PcbTextbox, textbox, modify_textbox;
            Table = 9, TableId, tables, crate::pcb_table::PcbTable, table, modify_table;
            Tablecell = 10, TablecellId, tablecells, crate::pcb_tablecell::PcbTablecell, tablecell, modify_tablecell;
            Dimension = 11, DimensionId, dimensions, crate::pcb_dimension::PcbDimension, dimension, modify_dimension;
            Barcode = 12, BarcodeId, barcodes, crate::pcb_barcode::PcbBarcode, barcode, modify_barcode;
            Image = 13, ImageId, images, crate::pcb_reference_image::PcbReferenceImage, image, modify_image;
            Target = 14, TargetId, targets, crate::pcb_target::PcbTarget, target, modify_target;
            Point = 15, PointId, points, crate::pcb_point::PcbPoint, point, modify_point;
            Group = 16, GroupId, groups, crate::pcb_group::PcbGroup, group, modify_group;
            Generator = 17, GeneratorId, generators, crate::pcb_generator::PcbGenerator, generator, modify_generator;
            Marker = 18, MarkerId, markers, crate::pcb_marker::PcbMarker, marker, modify_marker;
            Constraint = 19, ConstraintId, constraints, crate::pcb_constraint::PcbConstraint, constraint, modify_constraint;
            Body3d = 20, Body3dId, bodies, crate::pcb_3d_body::Pcb3dBody, body3d, modify_body3d;
            GridItem = 21, GridItemId, grid_items, crate::pcb_grid_item::PcbGridItem, grid_item, modify_grid_item;
            DrillMap = 22, DrillMapId, drill_maps, crate::pcb_drill_map::PcbDrillMap, drill_map, modify_drill_map;
        }
    };
}

/// `ids.rs` part: `ItemKind`, handle aliases, `ArenaItem` impls.
macro_rules! gen_ids {
    ( { $($extra:tt)* }
      $( $kind:ident = $disc:literal, $handle:ident, $arena:ident, $ty:ty, $getter:ident, $modify:ident; )* ) => {
        /// Which arena a handle belongs to (`board-model-types.md` section 0). Values are part
        /// of the wasm ABI: append only.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
        #[repr(u8)]
        pub enum ItemKind {
            $( #[doc = concat!("Arena `", stringify!($arena), "`.")] $kind = $disc, )*
            /// Not an arena: `index` is the net code (lets `ResolveItem` return a NETINFO_ITEM).
            Net = 23,
        }

        impl ItemKind {
            /// Every kind, in discriminant order.
            pub const ALL: &'static [ItemKind] = &[ $( ItemKind::$kind, )* ItemKind::Net ];

            /// The kind with discriminant `value`, if any.
            pub fn from_u8(value: u8) -> Option<ItemKind> {
                match value {
                    $( $disc => Some(ItemKind::$kind), )*
                    23 => Some(ItemKind::Net),
                    _ => None,
                }
            }

            /// The Rust name of the kind (diagnostics).
            pub fn name(self) -> &'static str {
                match self {
                    $( ItemKind::$kind => stringify!($kind), )*
                    ItemKind::Net => "Net",
                }
            }
        }

        $(
            #[doc = concat!("Typed handle of an item in `Board::", stringify!($arena), "`.")]
            pub type $handle = Id<$ty>;

            impl ArenaItem for $ty {
                const KIND: ItemKind = ItemKind::$kind;
            }
        )*
    };
}

/// `item_ref.rs` part: `AnyItem`, `AnyItemMut`.
macro_rules! gen_any_item {
    ( { $($extra:tt)* }
      $( $kind:ident = $disc:literal, $handle:ident, $arena:ident, $ty:ty, $getter:ident, $modify:ident; )* ) => {
        /// Resolved shared reference, for `match` (= `switch( Type() )` + `static_cast`): one
        /// variant per `ItemKind` (`board-model.md` 6.3).
        #[derive(Clone, Copy)]
        pub enum AnyItem<'a> {
            $( #[doc = concat!("`", stringify!($ty), "`.")] $kind(&'a $ty), )*
            /// A net (`NETINFO_ITEM`), not an arena item.
            Net(&'a crate::netinfo::NetinfoItem),
        }

        /// The `&mut` twin of [`AnyItem`] (no `Net`: nets are edited through `Board`).
        pub enum AnyItemMut<'a> {
            $( #[doc = concat!("`", stringify!($ty), "`.")] $kind(&'a mut $ty), )*
        }

        impl<'a> AnyItem<'a> {
            /// The kind of the referenced item.
            pub fn kind(&self) -> ItemKind {
                match self {
                    $( AnyItem::$kind(_) => ItemKind::$kind, )*
                    AnyItem::Net(_) => ItemKind::Net,
                }
            }

            /// The item as a trait object (the C++ `BOARD_ITEM*`).
            pub fn as_dyn(&self) -> &'a dyn BoardItem {
                match *self {
                    $( AnyItem::$kind(item) => item, )*
                    AnyItem::Net(item) => item,
                }
            }

            /// `EDA_ITEM` / `BOARD_ITEM` data members of the item.
            pub fn base(&self) -> &'a BoardItemBase {
                match *self {
                    $( AnyItem::$kind(item) => BoardItem::base(item), )*
                    AnyItem::Net(item) => BoardItem::base(item),
                }
            }
        }

        impl<'a> AnyItemMut<'a> {
            /// The kind of the referenced item.
            pub fn kind(&self) -> ItemKind {
                match self {
                    $( AnyItemMut::$kind(_) => ItemKind::$kind, )*
                }
            }

            /// The item as a mutable trait object.
            pub fn as_dyn_mut(&mut self) -> &mut dyn BoardItem {
                match self {
                    $( AnyItemMut::$kind(item) => &mut **item, )*
                }
            }

            /// Shared view of the same item.
            pub fn as_any(&self) -> AnyItem<'_> {
                match self {
                    $( AnyItemMut::$kind(item) => AnyItem::$kind(&**item), )*
                }
            }
        }

        $(
            impl<'a> From<&'a $ty> for AnyItem<'a> {
                fn from(item: &'a $ty) -> Self {
                    AnyItem::$kind(item)
                }
            }
        )*
    };
}

/// `board/mod.rs` part: the `Board` struct (arena fields first, then the tokens passed in),
/// `Index` impls, typed getters, `get`, `contains`, `modify_*`, per-arena invariant checks.
macro_rules! gen_board {
    ( { $(#[$meta:meta])* $vis:vis struct $name:ident { $($body:tt)* } }
      $( $kind:ident = $disc:literal, $handle:ident, $arena:ident, $ty:ty, $getter:ident, $modify:ident; )* ) => {
        $(#[$meta])*
        $vis struct $name {
            // ---- item arenas (generated by `board_items!`, board-model.md 2) ----
            $(
                #[doc = concat!("Arena of every `", stringify!($ty), "` of the board (slot order is meaningless).")]
                pub(crate) $arena: Arena<$ty>,
            )*
            $($body)*
        }

        $(
            impl ArenaOf<$ty> for $name {
                fn arena(&self) -> &Arena<$ty> {
                    &self.$arena
                }

                fn arena_mut(&mut self) -> &mut Arena<$ty> {
                    &mut self.$arena
                }
            }

            impl std::ops::Index<crate::ids::$handle> for $name {
                type Output = $ty;

                /// Panics on a stale handle; use the typed getter for handles that crossed a
                /// boundary (`board-model.md` 3).
                fn index(&self, id: crate::ids::$handle) -> &$ty {
                    match self.$arena.get(id) {
                        Some(item) => item,
                        None => panic!(concat!("stale ", stringify!($handle), " {:?}"), id),
                    }
                }
            }
        )*

        impl $name {
            $(
                #[doc = concat!("The `", stringify!($ty), "` of `id`; `None` for a stale handle.")]
                pub fn $getter(&self, id: crate::ids::$handle) -> Option<&$ty> {
                    self.$arena.get(id)
                }

                #[doc = concat!("Edit one `", stringify!($ty), "` (`board-model.md` 8.2): ")]
                /// invalidates its caches, runs `f` with the item and a `MutEnv` snapshot,
                /// invalidates the caches again (so `f` may read derived data before it
                /// writes, or store a value that carries caches), re-stamps `base.id`,
                /// `base.parent` and `base.group` (so `f` may assign a whole new value: the
                /// three are relations that only `Board` writes; a changed UUID is re-bound
                /// in the index like `BOARD_ITEM::SetUuid`), bumps the time stamp and
                /// invalidates the parent footprint's geometry caches. `None` for a stale
                /// handle. Child and member lists (`Footprint`, `PcbTable`, `PcbGroup`) are
                /// not protected: do not replace a container wholesale.
                pub fn $modify<R>(&mut self, id: crate::ids::$handle, f: impl FnOnce(&mut $ty, &MutEnv) -> R) -> Option<R> {
                    let env = self.mut_env_refreshed(id.into())?;
                    let item = self.$arena.get_mut(id)?;
                    let parent = BoardItem::base(item).parent;
                    let group = BoardItem::base(item).group;
                    let old_uuid = BoardItem::base(item).uuid;
                    Invalidate::invalidate_caches(item);
                    let result = f(item, &env);
                    Invalidate::invalidate_caches(item);
                    let base = BoardItem::base_mut(item);
                    base.id = Some(id.into());
                    base.parent = parent;
                    base.group = group;
                    let new_uuid = base.uuid;

                    if new_uuid != old_uuid {
                        self.rebind_uuid(id.into(), old_uuid, new_uuid);
                    }

                    self.after_modify(id.into(), parent);
                    Some(result)
                }
            )*

            /// `board.get(id)`: the item of a handle that may be stale (`None` then). Nets
            /// (`ItemKind::Net`, index = net code) resolve through the net list.
            pub fn get(&self, id: ItemId) -> Option<AnyItem<'_>> {
                match id.kind() {
                    $( ItemKind::$kind => self.$arena.get(id.typed::<$ty>()?).map(AnyItem::$kind), )*
                    ItemKind::Net => {
                        let code = NetCode(i32::try_from(id.index()).ok()?);
                        self.net_info.nets.get(&code).map(AnyItem::Net)
                    }
                }
            }

            /// True when `id` names a live item (or a known net).
            pub fn contains(&self, id: ItemId) -> bool {
                self.get(id).is_some()
            }

            /// Polymorphic `modify_*`: same steps, the item as `AnyItemMut`.
            pub fn modify<R>(&mut self, id: ItemId, f: impl FnOnce(AnyItemMut<'_>, &MutEnv) -> R) -> Option<R> {
                match id.kind() {
                    $( ItemKind::$kind => self.$modify(id.typed::<$ty>()?, |item, env| f(AnyItemMut::$kind(item), env)), )*
                    ItemKind::Net => None,
                }
            }

            /// Mutable item without the `modify` bookkeeping (crate-internal: cascades and the
            /// derived-state writers of `board-model.md` 8.4).
            #[allow(dead_code)] // for the cascades and derived-state writers (M1, M19, M18)
            pub(crate) fn get_mut_raw(&mut self, id: ItemId) -> Option<AnyItemMut<'_>> {
                match id.kind() {
                    $( ItemKind::$kind => self.$arena.get_mut(id.typed::<$ty>()?).map(AnyItemMut::$kind), )*
                    ItemKind::Net => None,
                }
            }

            /// Every live arena item as `(handle, base)` in kind order then slot order
            /// (bookkeeping only).
            pub(crate) fn for_each_live_item(&self, f: &mut dyn FnMut(ItemId, &BoardItemBase)) {
                $(
                    for (id, item) in self.$arena.iter() {
                        f(id.into(), BoardItem::base(item));
                    }
                )*
            }

            /// Number of live arena items (all kinds).
            #[allow(dead_code)] // for the container packages (M1)
            pub(crate) fn live_item_count(&self) -> usize {
                0 $( + self.$arena.len() )*
            }
        }
    };
}
