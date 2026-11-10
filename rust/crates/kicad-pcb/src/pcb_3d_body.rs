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

//! Port of `pcbnew/pcb_3d_body.h` and `pcbnew/pcb_3d_body.cpp` (`PCB_3D_BODY` and its
//! `EXTRUDED_3D_BODY` member), `board-model-types.md` section 10: a footprint child written as
//! `(model (type extruded) ...)`; data only, no 2D artwork.
//!
//! The members of `EXTRUDED_3D_BODY` (`m_extruded`) are flattened into the struct, as the
//! design defines it. Note that `PCB_3D_BODY::GetLayer` / `SetLayer` read and write
//! `m_extruded.m_layer` (`Pcb3dBody::layer`), not `BOARD_ITEM::m_layer` (`base.layer`, which
//! the constructor sets to `UNDEFINED_LAYER`). Behaviour: package M23.

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Color4D, KicadT, Kiid, KiidGenerator, PcbLayerId};
use kicad_geom::math::vector3::Vector3D;
use kicad_geom::{FlipDirection, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::item::BoardItemBase;
use crate::traits::{BoardItem, BoardItemMut, Invalidate};

/// `PCB_3D_BODY` + `EXTRUDED_3D_BODY`.
#[derive(Clone, Debug)]
pub struct Pcb3dBody {
    pub base: BoardItemBase,
    pub kind: Body3dKind, // m_kind
    pub height: i32,      // m_extruded.m_height
    pub standoff: i32,    // m_extruded.m_standoff
    /// m_extruded.m_layer (UNDEFINED = auto, UNSELECTED = pad_bbox).
    pub layer: PcbLayerId,
    pub color: Color4D,              // m_extruded.m_color
    pub material: ExtrusionMaterial, // m_extruded.m_material
    pub show: bool,                  // m_extruded.m_show
    pub scale: Vector3D,             // m_extruded.m_scale
    pub rotation: Vector3D,          // m_extruded.m_rotation
    pub offset: Vector3D,            // m_extruded.m_offset
}

/// `PCB_3D_BODY::KIND`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum Body3dKind {
    #[default]
    Extruded = 0,
    /// Reserved; not constructible yet.
    Model,
}

/// `EXTRUSION_MATERIAL`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ExtrusionMaterial {
    #[default]
    Plastic = 0,
    Matte,
    Metal,
    Copper,
}

impl ExtrusionMaterial {
    /// `EXTRUDED_3D_BODY::GetDefaultColor( aMaterial )`.
    pub fn default_color(self) -> Color4D {
        match self {
            ExtrusionMaterial::Plastic => Color4D::new(0.2, 0.2, 0.2, 1.0),
            ExtrusionMaterial::Matte => Color4D::new(0.4, 0.4, 0.4, 1.0),
            ExtrusionMaterial::Metal => Color4D::new(0.7, 0.7, 0.7, 1.0),
            ExtrusionMaterial::Copper => Color4D::new(0.72, 0.45, 0.2, 1.0),
        }
    }
}

// ---- constructors ------------------------------------------------------------------------

impl Default for Pcb3dBody {
    /// `PCB_3D_BODY( aParent )` (`pcb_3d_body.cpp`): `BOARD_ITEM( .., UNDEFINED_LAYER )`,
    /// `m_kind = EXTRUDED` and the in-class initialisers of `EXTRUDED_3D_BODY`
    /// (`pcb_3d_body.h`): height 0, standoff 0, layer `UNDEFINED_LAYER`, color
    /// `COLOR4D::UNSPECIFIED`, material `PLASTIC`, shown, scale (1, 1, 1), rotation and offset
    /// (0, 0, 0); nil UUID.
    fn default() -> Self {
        Pcb3dBody {
            base: BoardItemBase::with_layer(PcbLayerId::UndefinedLayer),
            kind: Body3dKind::Extruded,
            height: 0,
            standoff: 0,
            layer: PcbLayerId::UndefinedLayer,
            color: Color4D::UNSPECIFIED,
            material: ExtrusionMaterial::Plastic,
            show: true,
            scale: Vector3D::new(1.0, 1.0, 1.0),
            rotation: Vector3D::new(0.0, 0.0, 0.0),
            offset: Vector3D::new(0.0, 0.0, 0.0),
        }
    }
}

impl Pcb3dBody {
    /// `new PCB_3D_BODY( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut body = Pcb3dBody::default();
        body.base.uuid = Kiid::new(kiid_generator);
        body
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for Pcb3dBody {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for Pcb3dBody {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::Pcb3dBodyT
    }
    fn class_name(&self) -> &'static str {
        "PCB_3D_BODY"
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        Pcb3dBody::layer(self, ctx)
    }
    fn is_layer_agnostic(&self) -> bool {
        Pcb3dBody::is_layer_agnostic(self)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        Pcb3dBody::item_description(self, ctx, units, full)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        Pcb3dBody::friendly_name(self, ctx)
    }
}

impl BoardItemMut for Pcb3dBody {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        Pcb3dBody::flip(self, env, centre, dir)
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        Pcb3dBody::set_layer(self, env, layer)
    }
}

// ---- inherent methods (package M23) -------------------------------------------------------

impl Pcb3dBody {
    /// `GetKind()`.
    pub fn kind(&self) -> Body3dKind {
        self.kind
    }

    /// `PCB_3D_BODY::GetLayer()`: `m_extruded.m_layer`.
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.layer
    }

    /// `PCB_3D_BODY::SetLayer()`: `m_extruded.m_layer = aLayer`.
    pub fn set_layer(&mut self, _env: &MutEnv, layer: PcbLayerId) {
        self.layer = layer;
    }

    /// `PCB_3D_BODY::IsLayerAgnostic()`: true.
    pub fn is_layer_agnostic(&self) -> bool {
        true
    }

    /// `PCB_3D_BODY::GetFriendlyName()`: "3D Body".
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        "3D Body".to_string()
    }

    /// `PCB_3D_BODY::GetItemDescription()`: "3D Body".
    pub fn item_description(
        &self,
        _ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        "3D Body".to_string()
    }

    /// `PCB_3D_BODY::Flip()`: an explicit layer goes to the board's flipped layer
    /// (`BOARD::FlipLayer`, or `::FlipLayer` without a board); `UNDEFINED_LAYER` stays.
    pub fn flip(&mut self, env: &MutEnv, _centre: Vector2I, _dir: FlipDirection) {
        if self.layer == PcbLayerId::UndefinedLayer {
            return;
        }

        // board->FlipLayer( m_layer ), ::FlipLayer( m_layer ) without a board: the layer table
        // of the environment is the board's or the standard one.
        self.layer = env.layers.flip_layer(self.layer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PCB_3D_BODY::Flip`: an explicit layer flips, UNDEFINED_LAYER (auto) stays.
    #[test]
    fn body_flip() {
        let env = MutEnv::detached();
        let mut b = Pcb3dBody::default();
        b.layer = PcbLayerId::FSilkS;
        b.flip(&env, Vector2I::new(0, 0), FlipDirection::LeftRight);
        assert_eq!(b.layer, PcbLayerId::BSilkS);
        b.layer = PcbLayerId::UndefinedLayer;
        b.flip(&env, Vector2I::new(0, 0), FlipDirection::LeftRight);
        assert_eq!(b.layer, PcbLayerId::UndefinedLayer);
    }
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_3D_BODY::PCB_3D_BODY( aParent )` (pcbnew/pcb_3d_body.cpp) and the
    /// `EXTRUDED_3D_BODY` initialisers (pcbnew/pcb_3d_body.h).
    #[test]
    fn body_constructor_defaults() {
        let b = Pcb3dBody::default();
        assert_eq!(b.base.layer, PcbLayerId::UndefinedLayer);
        assert_eq!(b.kind(), Body3dKind::Extruded);
        assert_eq!((b.height, b.standoff), (0, 0));
        assert_eq!(b.layer, PcbLayerId::UndefinedLayer);
        assert_eq!(b.color, Color4D::UNSPECIFIED);
        assert_eq!(b.material, ExtrusionMaterial::Plastic);
        assert!(b.show);
        assert_eq!(b.scale, Vector3D::new(1.0, 1.0, 1.0));
        assert_eq!(b.rotation, Vector3D::new(0.0, 0.0, 0.0));
        assert_eq!(b.offset, Vector3D::new(0.0, 0.0, 0.0));
        assert!(b.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&b), "PCB_3D_BODY");
        assert_eq!(
            ExtrusionMaterial::Copper.default_color(),
            Color4D::new(0.72, 0.45, 0.2, 1.0)
        );
    }

    /// GetLayer / SetLayer use the extruded layer, through the traits, on a footprint child.
    #[test]
    fn body_layer_is_the_extruded_layer() {
        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Body3d).typed::<Pcb3dBody>().unwrap();
        let mut b = fx.board[id].clone();
        BoardItemMut::set_layer(&mut b, &MutEnv::detached(), PcbLayerId::BSilkS);
        assert_eq!(b.base.layer, PcbLayerId::UndefinedLayer);
        fx.set_child(id, b);
        fx.board.check_invariants().unwrap();
        let ctx = BoardCtx::bare(&fx.board);
        let b: &dyn BoardItem = &fx.board[id];
        assert_eq!(b.layer(&ctx), PcbLayerId::BSilkS);
        assert!(b.is_layer_agnostic());
        assert_eq!(b.friendly_name(&ctx), "3D Body");
    }
}
