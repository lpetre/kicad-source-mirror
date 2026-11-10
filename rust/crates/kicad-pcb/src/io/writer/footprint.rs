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
//! PCBIO-06: `format( const FOOTPRINT*, bool aInLibFile )` and `formatInitialComments`
//! (W:1452-1884).
//!
//! The children are written in the order of the C++ `std::set`s built with the model
//! comparators (`FOOTPRINT::cmp_drawings`, `PCB_POINT::cmp_points`, `FOOTPRINT::cmp_pads`,
//! `FOOTPRINT::cmp_zones`, `PCB_GROUP::ptr_cmp`), reproduced by [`sorted_set`].

use kicad_core::eda_units::format_angle;
use kicad_core::string_utils::format_double2_str;
use kicad_core::{Color4D, Lset, PcbLayerId};
use kicad_sexpr::{IoError, format_bool, format_custom_properties, format_uuid};

use super::{
    CTL_FOR_BOARD, CTL_OMIT_AT, CTL_OMIT_FOOTPRINT_VERSION, CTL_OMIT_INITIAL_COMMENTS,
    CTL_OMIT_LIBNAME, CTL_OMIT_PATH, CTL_OMIT_UUIDS, PcbWriter, format_iu, format_iu_pt,
    sorted_set,
};
use crate::board_item::{board_item_ptr_cmp, is_locked};
use crate::footprint::FootprintStackup;
use crate::footprint::compare::{footprint_cmp_drawings, footprint_cmp_pads, footprint_cmp_zones};
use crate::footprint::footprint_attr::{
    FP_BOARD_ONLY, FP_DNP, FP_EXCLUDE_FROM_BOM, FP_EXCLUDE_FROM_POS_FILES, FP_EXCLUDE_FROM_SIM,
    FP_SMD, FP_THROUGH_HOLE,
};
use crate::ids::{FootprintId, ItemId};
use crate::io::SEXPR_BOARD_FILE_VERSION;
use crate::pcb_3d_body::ExtrusionMaterial;
use crate::pcb_field::FieldT;
use crate::pcb_point::pcb_point_cmp_points;
use crate::zone_settings::ZoneConnection;

impl PcbWriter<'_> {
    /// Port of `format( const FOOTPRINT* aFootprint, bool aInLibFile )`: `aInLibFile` leaves the
    /// initial comments and version to the enclosing library file.
    pub(crate) fn format_footprint(
        &mut self,
        footprint: FootprintId,
        in_lib_file: bool,
    ) -> Result<(), IoError> {
        let src = self.src;
        let ctx = self.ctx;

        let Some(fp) = src.footprint(footprint) else {
            return Ok(());
        };

        if !in_lib_file {
            self.format_initial_comments(footprint)?;
        }

        if (self.ctl & CTL_OMIT_LIBNAME) != 0 {
            let name = self.out.quotes(fp.fpid().lib_item_name());
            self.out.print(format_args!("(footprint {name}"))?;
        } else {
            let name = self.out.quotes(&fp.fpid().format());
            self.out.print(format_args!("(footprint {name}"))?;
        }

        if !in_lib_file && (self.ctl & CTL_OMIT_FOOTPRINT_VERSION) == 0 {
            let version = self.out.quotew(&self.generator_version);
            self.out.print(format_args!(
                "(version {SEXPR_BOARD_FILE_VERSION}) (generator \"pcbnew\") \
                 (generator_version {version})"
            ))?;
        }

        if is_locked(fp, &ctx) {
            format_bool(&mut *self.out, "locked", true)?;
        }

        if fp.is_placed() {
            format_bool(&mut *self.out, "placed", true)?;
        }

        self.format_layer(fp.layer(&ctx), false)?;

        if (self.ctl & CTL_OMIT_UUIDS) == 0 {
            format_uuid(&mut *self.out, &fp.base.uuid.as_string())?;
        }

        if (self.ctl & CTL_OMIT_AT) == 0 {
            let transform = fp.transform();
            self.out.print(format_args!(
                "(transform (translate {}) (rotate {}) (scale {} {}))",
                format_iu_pt(fp.position(&ctx)),
                format_angle(fp.orientation().as_degrees()),
                format_double2_str(transform.scale_x()),
                format_double2_str(transform.scale_y())
            ))?;
        }

        if !fp.lib_description.is_empty() {
            let descr = self.out.quotew(&fp.lib_description);
            self.out.print(format_args!("(descr {descr})"))?;
        }

        if !fp.keywords.is_empty() {
            let tags = self.out.quotew(&fp.keywords);
            self.out.print(format_args!("(tags {tags})"))?;
        }

        if fp.height != 0 {
            self.out
                .print(format_args!("(height {})", format_iu(fp.height)))?;
        }

        for (field_id, field) in src.footprint_fields(footprint) {
            let name = self.out.quotew(&field.untranslated_name());
            let text = self.out.quotew(field.text.text.text());
            self.out.print(format_args!("(property {name} {text}"))?;

            self.format_field(field_id)?;

            self.out.print(format_args!(")"))?;
        }

        if let Some(comp_class) = fp.static_component_class()
            && !comp_class.is_empty()
        {
            self.out.print(format_args!("(component_classes"))?;

            for constituent in comp_class.constituent_names() {
                let name = self.out.quotew(constituent);
                self.out.print(format_args!("(class {name})"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        if !fp.filters.is_empty() {
            let filters = self.out.quotew(&fp.filters);
            self.out
                .print(format_args!("(property ki_fp_filters {filters})"))?;
        }

        if (self.ctl & CTL_OMIT_PATH) == 0 && !fp.path.is_empty() {
            let path = self.out.quotew(&fp.path.as_string());
            self.out.print(format_args!("(path {path})"))?;
        }

        if !fp.sheetname.is_empty() {
            let sheetname = self.out.quotew(&fp.sheetname);
            self.out.print(format_args!("(sheetname {sheetname})"))?;
        }

        if !fp.sheetfile.is_empty() {
            let sheetfile = self.out.quotew(&fp.sheetfile);
            self.out.print(format_args!("(sheetfile {sheetfile})"))?;
        }

        // Emit unit info for gate swapping metadata (flat pin list form)
        if !fp.unit_info.is_empty() {
            self.out.print(format_args!("(units"))?;

            for u in &fp.unit_info {
                let unit_name = self.out.quotew(&u.unit_name);
                self.out.print(format_args!("(unit (name {unit_name})"))?;
                self.out.print(format_args!("(pins"))?;

                for n in &u.pins {
                    let pin = self.out.quotew(n);
                    self.out.print(format_args!(" {pin}"))?;
                }

                self.out.print(format_args!(")"))?; // </pins>
                self.out.print(format_args!(")"))?; // </unit>
            }

            self.out.print(format_args!(")"))?; // </units>
        }

        if let Some(margin) = fp.solder_mask_margin {
            self.out
                .print(format_args!("(solder_mask_margin {})", format_iu(margin)))?;
        }

        if let Some(margin) = fp.solder_paste_margin {
            self.out
                .print(format_args!("(solder_paste_margin {})", format_iu(margin)))?;
        }

        if let Some(ratio) = fp.solder_paste_margin_ratio {
            self.out.print(format_args!(
                "(solder_paste_margin_ratio {})",
                format_double2_str(ratio)
            ))?;
        }

        if let Some(clearance) = fp.local_clearance() {
            self.out
                .print(format_args!("(clearance {})", format_iu(clearance)))?;
        }

        if fp.local_zone_connection() != ZoneConnection::Inherited {
            self.out.print(format_args!(
                "(zone_connect {})",
                fp.local_zone_connection() as i32
            ))?;
        }

        // Attributes
        let attributes = fp.attributes();

        if attributes != 0 || fp.allow_missing_courtyard() || fp.allow_solder_mask_bridges() {
            self.out.print(format_args!("(attr"))?;

            if attributes & FP_SMD != 0 {
                self.out.print(format_args!(" smd"))?;
            }

            if attributes & FP_THROUGH_HOLE != 0 {
                self.out.print(format_args!(" through_hole"))?;
            }

            if attributes & FP_BOARD_ONLY != 0 {
                self.out.print(format_args!(" board_only"))?;
            }

            if attributes & FP_EXCLUDE_FROM_POS_FILES != 0 {
                self.out.print(format_args!(" exclude_from_pos_files"))?;
            }

            if attributes & FP_EXCLUDE_FROM_BOM != 0 {
                self.out.print(format_args!(" exclude_from_bom"))?;
            }

            if attributes & FP_EXCLUDE_FROM_SIM != 0 {
                self.out.print(format_args!(" exclude_from_sim"))?;
            }

            if fp.allow_missing_courtyard() {
                self.out.print(format_args!(" allow_missing_courtyard"))?;
            }

            if attributes & FP_DNP != 0 {
                self.out.print(format_args!(" dnp"))?;
            }

            if fp.allow_solder_mask_bridges() {
                self.out.print(format_args!(" allow_soldermask_bridges"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        // Expand inner layers is the default stackup mode
        if fp.stackup_mode != FootprintStackup::ExpandInnerLayers {
            self.out.print(format_args!("(stackup"))?;

            for layer in fp.stackup_layers.seq() {
                let canonical_name = self.out.quotew(&Lset::name(layer));
                self.out.print(format_args!("(layer {canonical_name})"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        if fp.private_layers.any() {
            self.out.print(format_args!("(private_layers"))?;

            for layer in fp.private_layers.seq() {
                let canonical_name = self.out.quotew(&Lset::name(layer));
                self.out.print(format_args!(" {canonical_name}"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        if fp.is_net_tie() {
            self.out.print(format_args!("(net_tie_pad_groups"))?;

            for group in &fp.net_tie_pad_groups {
                let group = self.out.quotew(group);
                self.out.print(format_args!(" {group}"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        format_bool(
            &mut *self.out,
            "duplicate_pad_numbers_are_jumpers",
            fp.duplicate_pad_numbers_are_jumpers(),
        )?;

        let jumper_groups = &fp.jumper_pad_groups;

        if !jumper_groups.is_empty() {
            self.out.print(format_args!("(jumper_pad_groups"))?;

            for group in jumper_groups.all() {
                self.out.print(format_args!("("))?;

                for pad_name in group.names() {
                    let pad_name = self.out.quotew(pad_name);
                    self.out.print(format_args!("{pad_name} "))?;
                }

                self.out.print(format_args!(")"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        // Format( &aFootprint->Reference() ) / Format( &aFootprint->Value() ): the dispatch
        // writes nothing for a PCB_FIELD (fields are written as properties above).
        for field_type in [FieldT::Reference, FieldT::Value] {
            if let Some(field) = fp.field(&ctx, field_type) {
                self.format_item(field.into())?;
            }
        }

        let sorted_pads: Vec<ItemId> = sorted_set(
            src.footprint_pads(footprint)
                .map(|(id, _)| ItemId::from(id)),
            |a, b| footprint_cmp_pads(&ctx, *a, *b),
        );
        let sorted_drawings: Vec<ItemId> = sorted_set(
            src.footprint_drawings(footprint).map(|(id, _)| id),
            |a, b| footprint_cmp_drawings(&ctx, *a, *b),
        );
        let sorted_points: Vec<ItemId> = sorted_set(
            src.footprint_points(footprint)
                .map(|(id, _)| ItemId::from(id)),
            |a, b| pcb_point_cmp_points(&ctx, *a, *b),
        );
        let sorted_zones: Vec<ItemId> = sorted_set(
            src.footprint_zones(footprint)
                .map(|(id, _)| ItemId::from(id)),
            |a, b| footprint_cmp_zones(&ctx, *a, *b),
        );
        let sorted_groups: Vec<ItemId> = sorted_set(
            src.footprint_groups(footprint)
                .map(|(id, _)| ItemId::from(id)),
            |a, b| board_item_ptr_cmp(&ctx, *a, *b),
        );
        let sorted_constraints: Vec<ItemId> = sorted_set(
            src.footprint_constraints(footprint)
                .map(|(id, _)| ItemId::from(id)),
            |a, b| board_item_ptr_cmp(&ctx, *a, *b),
        );

        // Save drawing elements.

        for &gr in &sorted_drawings {
            self.format_item(gr)?;
        }

        for &point in &sorted_points {
            self.format_item(point)?;
        }

        // Save pads.
        for &pad in &sorted_pads {
            self.format_item(pad)?;
        }

        // Save zones.
        for &zone in &sorted_zones {
            self.format_item(zone)?;
        }

        // Save groups.
        for &group in &sorted_groups {
            self.format_item(group)?;
        }

        // Save geometric constraints, after the items they reference.
        for &constraint in &sorted_constraints {
            self.format_item(constraint)?;
        }

        // Save variants.
        let base_dnp = fp.is_dnp();
        let base_excluded_from_bom = fp.is_excluded_from_bom();
        let base_excluded_from_sim = fp.is_excluded_from_sim();
        let base_excluded_from_pos_files = fp.is_excluded_from_pos_files();

        for (variant_name, variant) in &fp.variants {
            let name = self.out.quotew(variant_name.as_str());
            self.out.print(format_args!("(variant (name {name})"))?;

            if variant.dnp != base_dnp {
                format_bool(&mut *self.out, "dnp", variant.dnp)?;
            }

            if variant.excluded_from_bom != base_excluded_from_bom {
                format_bool(
                    &mut *self.out,
                    "exclude_from_bom",
                    variant.excluded_from_bom,
                )?;
            }

            if variant.excluded_from_sim != base_excluded_from_sim {
                format_bool(
                    &mut *self.out,
                    "exclude_from_sim",
                    variant.excluded_from_sim,
                )?;
            }

            if variant.excluded_from_pos_files != base_excluded_from_pos_files {
                format_bool(
                    &mut *self.out,
                    "exclude_from_pos_files",
                    variant.excluded_from_pos_files,
                )?;
            }

            for (field_name, field_value) in &variant.fields {
                let base_value = fp
                    .field_by_name(&ctx, field_name)
                    .and_then(|id| src.field(id))
                    .map(|f| f.text.text.text().to_string())
                    .unwrap_or_default();

                if *field_value == base_value {
                    continue;
                }

                let name = self.out.quotew(field_name);
                let value = self.out.quotew(field_value);
                self.out
                    .print(format_args!("(field (name {name}) (value {value}))"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        format_bool(
            &mut *self.out,
            "embedded_fonts",
            fp.embedded_files.are_fonts_embedded(),
        )?;

        if !fp.embedded_files.is_empty() {
            fp.embedded_files
                .write_embedded_files(&mut *self.out, (self.ctl & CTL_FOR_BOARD) == 0)?;
        }

        // Save extruded 3D body info.
        for (_, body) in src.footprint_bodies(footprint) {
            if body.height <= 0 {
                continue;
            }

            self.out.print(format_args!("(model"))?;
            self.out.print(format_args!("(type extruded)"))?;
            format_uuid(&mut *self.out, &body.base.uuid.as_string())?;
            format_bool(&mut *self.out, "hide", !body.show)?;
            self.out
                .print(format_args!("(overall_height {})", format_iu(body.height)))?;
            self.out
                .print(format_args!("(body_pcb_gap {})", format_iu(body.standoff)))?;

            if body.layer == PcbLayerId::UnselectedLayer {
                self.out.print(format_args!("(layer pad_bbox)"))?;
            } else if body.layer != PcbLayerId::UndefinedLayer {
                let layer = self.out.quotew(&Lset::name(body.layer));
                self.out.print(format_args!("(layer {layer})"))?;
            } else {
                self.out.print(format_args!("(layer auto)"))?;
            }

            {
                let mat_name = match body.material {
                    ExtrusionMaterial::Plastic => "plastic",
                    ExtrusionMaterial::Matte => "matte",
                    ExtrusionMaterial::Metal => "metal",
                    ExtrusionMaterial::Copper => "copper",
                };
                self.out.print(format_args!("(material {mat_name})"))?;
            }

            if body.color != Color4D::UNSPECIFIED {
                self.out.print(format_args!(
                    "(color {} {} {} {})",
                    format_double2_str(body.color.r),
                    format_double2_str(body.color.g),
                    format_double2_str(body.color.b),
                    format_double2_str(body.color.a)
                ))?;
            } else {
                self.out.print(format_args!("(color unspecified)"))?;
            }

            self.out.print(format_args!(
                "(offset (xyz {} {} {}))",
                format_double2_str(body.offset.x),
                format_double2_str(body.offset.y),
                format_double2_str(body.offset.z)
            ))?;

            self.out.print(format_args!(
                "(scale (xyz {} {} {}))",
                format_double2_str(body.scale.x),
                format_double2_str(body.scale.y),
                format_double2_str(body.scale.z)
            ))?;

            self.out.print(format_args!(
                "(rotate (xyz {} {} {}))",
                format_double2_str(body.rotation.x),
                format_double2_str(body.rotation.y),
                format_double2_str(body.rotation.z)
            ))?;

            self.out.print(format_args!(")"))?;
        }

        // Save 3D info.
        for model in &fp.models {
            if model.filename.is_empty() {
                continue;
            }

            let filename = self.out.quotew(&model.filename);
            self.out.print(format_args!("(model {filename}"))?;

            if !model.show {
                format_bool(&mut *self.out, "hide", !model.show)?;
            }

            if model.opacity != 1.0 {
                // fmt::format( "(opacity {:.4f})", m_Opacity ): fixed notation with four
                // decimals, correctly rounded like Rust's `{:.4}`.
                self.out
                    .print(format_args!("(opacity {:.4})", model.opacity))?;
            }

            self.out.print(format_args!(
                "(offset (xyz {} {} {}))",
                format_double2_str(model.offset.x),
                format_double2_str(model.offset.y),
                format_double2_str(model.offset.z)
            ))?;

            self.out.print(format_args!(
                "(scale (xyz {} {} {}))",
                format_double2_str(model.scale.x),
                format_double2_str(model.scale.y),
                format_double2_str(model.scale.z)
            ))?;

            self.out.print(format_args!(
                "(rotate (xyz {} {} {}))",
                format_double2_str(model.rotation.x),
                format_double2_str(model.rotation.y),
                format_double2_str(model.rotation.z)
            ))?;

            self.out.print(format_args!(")"))?;
        }

        format_custom_properties(&mut *self.out, &fp.base.custom_properties)?;
        self.out.print(format_args!(")"))?;

        Ok(())
    }

    /// Port of `formatInitialComments( const FOOTPRINT* )`: the verbatim `#...` lines (not with
    /// `CTL_OMIT_INITIAL_COMMENTS`).
    pub(crate) fn format_initial_comments(
        &mut self,
        footprint: FootprintId,
    ) -> Result<(), IoError> {
        if (self.ctl & CTL_OMIT_INITIAL_COMMENTS) == 0 {
            let Some(fp) = self.src.footprint(footprint) else {
                return Ok(());
            };

            if let Some(initial_comments) = &fp.initial_comments {
                for comment in initial_comments {
                    self.out.print(format_args!("{comment}\n"))?;
                }
            }
        }

        Ok(())
    }
}
