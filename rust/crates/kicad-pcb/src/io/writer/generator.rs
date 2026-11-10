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

//! Port of the writer part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` for package
//! PCBIO-13: `format( const PCB_GENERATOR* )` and `format( const PCB_CONSTRAINT* )` (W:3053-3238).
//!
//! The generator property map written here is what `GetProperties()` returns: the parser
//! normalises the file map through `PcbGenerator::normalize_properties` (the subclass
//! `SetProperties` / `GetProperties` round trip, M23), so the map is written as stored.

use kicad_core::{Lset, PCB_IU_SCALE};
use kicad_core::string_utils::format_double2_str;
use kicad_sexpr::IoError;
use kicad_sexpr::format_g;
use kicad_sexpr::{format_bool, format_custom_properties, format_uuid};

use super::{PcbWriter, format_iu_pt};
use crate::ids::{ConstraintId, GeneratorId, ItemId};
use crate::item_ref::OwnedItem;
use crate::pcb_constraint::{
    ConstraintAnchor, constraint_anchor_token, constraint_type_token, constraint_value_is_length,
};
use crate::pcb_generator::{
    GENERATOR_TYPE_TUNING_PATTERN, GENERATOR_TYPE_VIA_STACK, GeneratorProp,
};

impl PcbWriter<'_> {
    /// Port of `format( const PCB_GENERATOR* aGenerator )`: `(generated (uuid ..) (type t)
    /// (name "n") (layer "L") [(locked yes)] (key value)* [(templates ..)] (members ..)
    /// (custom_property ..)*)`.
    pub(crate) fn format_generator(&mut self, generator: GeneratorId) -> Result<(), IoError> {
        let Some(g) = self.src.generators.get(generator) else {
            return Ok(());
        };
        let item: ItemId = generator.into();

        // Some conditions appear to still be creating ghost tuning patterns.  Don't save them.
        // A microvia stack with no members is the same kind of ghost: it reloads with no size,
        // so it can be neither seen nor picked, and with no net to rebuild its vias on.
        if g.group.members.is_empty()
            && (g.generator_type() == GENERATOR_TYPE_TUNING_PATTERN
                || g.generator_type() == GENERATOR_TYPE_VIA_STACK)
        {
            return Ok(());
        }

        let item_ref = self.ctx.item(item);

        self.out.print(format_args!("(generated"))?;

        format_uuid(self.out, &g.group.base.uuid.as_string())?;

        // GetLayer() is virtual (the tuning pattern / via stack overrides).
        let layer = match &item_ref {
            Some(r) => r.layer(),
            None => g.group.base.layer,
        };

        // TO_UTF8( GetGeneratorType() ): written bare.
        let name = self.out.quotew(g.group.name.as_str());
        let layer_name = self.out.quotew(&Lset::name(layer));
        self.out.print(format_args!(
            "(type {}) (name {}) (layer {})",
            g.generator_type(),
            name,
            layer_name
        ))?;

        if item_ref.as_ref().is_some_and(|r| r.is_locked()) {
            format_bool(self.out, "locked", true)?;
        }

        for (key, value) in &g.properties {
            match value {
                GeneratorProp::Number(val) => {
                    let buf = format_g(*val, 10);

                    // Don't quote numbers
                    self.out.print(format_args!("({key} {buf})"))?;
                }
                GeneratorProp::Bool(val) => {
                    format_bool(self.out, key, *val)?;
                }
                GeneratorProp::Vector2I(val) => {
                    self.out
                        .print(format_args!("({key} (xy {}))", format_iu_pt(*val)))?;
                }
                GeneratorProp::LineChain(val) => {
                    self.out.print(format_args!("({key} "))?;
                    self.format_poly_pts(val, None)?;
                    self.out.print(format_args!(")"))?;
                }
                GeneratorProp::Cells(val) => {
                    // Unitless integer pairs
                    self.out.print(format_args!("({key} (cells"))?;

                    for cell in val {
                        self.out.print(format_args!("(ij {} {})", cell.x, cell.y))?;
                    }

                    self.out.print(format_args!("))"))?;
                }
                GeneratorProp::String(val) => {
                    let quoted = self.out.quotew(val);
                    self.out.print(format_args!("({key} {quoted})"))?;
                }
            }
        }

        // GetTemplateItems()
        if !g.templates.is_empty() {
            self.out.print(format_args!("(templates"))?;

            for (name, template) in &g.templates {
                let quoted = self.out.quotew(name);
                self.out
                    .print(format_args!("(template (name {quoted})"))?;
                self.format_template_item(template)?;
                self.out.print(format_args!(")"))?; // Close `template` token.
            }

            self.out.print(format_args!(")"))?; // Close `templates` token.
        }

        let mut member_ids: Vec<String> = g
            .group
            .members
            .iter()
            .filter_map(|&member| self.src.uuid_of(member))
            .map(|uuid| uuid.as_string())
            .collect();

        // wxArrayString::Sort(): wxString::Cmp, i.e. code point (= UTF-8 byte) order.
        member_ids.sort();

        self.out.print(format_args!("(members"))?;

        for member_id in &member_ids {
            let quoted = self.out.quotew(member_id);
            self.out.print(format_args!(" {quoted}"))?;
        }

        self.out.print(format_args!(")"))?; // Close `members` token.

        format_custom_properties(self.out, &g.group.base.custom_properties)?;
        self.out.print(format_args!(")"))?; // Close `generated` token.

        Ok(())
    }

    /// `Format( item )` of a generator template item (`PCB_VIA_STITCH`'s via template), which
    /// is owned by the generator and not stored in the board arena.
    ///
    /// The track formatter of PCBIO-03 (`format_track`) only accepts arena handles; until it
    /// gains a variant taking a `&Track` (change request of PCBIO-13) the template is a
    /// staging skip: "port pending: generator template (PCBIO-13)", or nothing written with
    /// `allow_pending`.
    fn format_template_item(&mut self, template: &OwnedItem) -> Result<(), IoError> {
        let _ = template;
        self.pending("generator template", "PCBIO-13")
    }

    /// Port of `format( const PCB_CONSTRAINT* aConstraint )`: `(constraint (type t) (uuid ..)
    /// (members (member "uuid" anchor [index]) ..) [(value v)] [(driving no)]
    /// (custom_property ..)*)`; a constraint without members is not written.
    pub(crate) fn format_constraint(&mut self, constraint: ConstraintId) -> Result<(), IoError> {
        let Some(c) = self.src.constraints.get(constraint) else {
            return Ok(());
        };

        let members = c.members();

        if members.is_empty() {
            return Ok(());
        }

        // Members are KIID references (not pointers), so unlike format(PCB_GROUP*) there is
        // no use-after-free risk: every member is written verbatim, including one whose item
        // was deleted, so the constraint round-trips in its error state rather than silently
        // losing the reference.
        self.out.print(format_args!(
            "(constraint (type {})",
            constraint_type_token(c.constraint_type())
        ))?;

        format_uuid(self.out, &c.base.uuid.as_string())?;

        self.out.print(format_args!("(members"))?;

        for member in members {
            let quoted = self.out.quotew(&member.item.as_string());

            // Only VERTEX carries an ordinal others stay two-token
            if member.anchor == ConstraintAnchor::Vertex {
                self.out.print(format_args!(
                    "(member {} {} {})",
                    quoted,
                    constraint_anchor_token(member.anchor),
                    member.index
                ))?;
            } else {
                self.out.print(format_args!(
                    "(member {} {})",
                    quoted,
                    constraint_anchor_token(member.anchor)
                ))?;
            }
        }

        self.out.print(format_args!(")"))?; // Close `members` token.

        if let Some(mut value) = c.value() {
            // Length/radius values are stored in IU but written in mm like every other
            // dimension; angle values are written verbatim in degrees.
            if constraint_value_is_length(c.constraint_type()) {
                value /= PCB_IU_SCALE.iu_per_mm;
            }

            self.out
                .print(format_args!("(value {})", format_double2_str(value)))?;
        }

        if !c.is_driving() {
            format_bool(self.out, "driving", false)?;
        }

        format_custom_properties(self.out, &c.base.custom_properties)?;
        self.out.print(format_args!(")"))?; // Close `constraint` token.

        Ok(())
    }
}
