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
//! PCBIO-12: `format( const PCB_REFERENCE_IMAGE* )`, `format( const PCB_POINT* )`,
//! `format( const PCB_TARGET* )`, `format( const PCB_GRID_ITEM* )` (W:1334-1449),
//! `format( const PCB_BARCODE* )` (W:2552-2616), `formatTableData`, `format( const
//! PCB_DRILL_MAP* )`, `format( const PCB_GENERATED_TABLE* )`, `formatGeneratedTableExtras`,
//! `format( const PCB_TABLE* )` (W:2776-2999) and the static `formatDrillSpan` (W:683).

use kicad_core::eda_units::format_angle;
use kicad_core::{Lset, PCB_IU_SCALE};
use kicad_sexpr::{
    IoError, OutputFormatter, format_bool, format_custom_properties, format_g, format_stream_data,
    format_uuid,
};

use super::{PcbWriter, format_iu, format_iu_pt};
use crate::ids::{BarcodeId, DrillMapId, GridItemId, ImageId, PointId, TableId, TargetId};
use crate::pcb_barcode::{BarcodeEccT, BarcodeT};
use crate::pcb_drill_map::DrillSpan;
use crate::pcb_generated_table::{
    DrillChartFilter, GeneratedTableKind, GeneratedTableSchema, drill_chart_schema,
    generated_table_align_token, generated_table_units_token, stackup_table_schema,
};
use crate::pcb_grid_item::PcbGridType;
use crate::traits::BoardItem;

/// `aTable->Schema()`: `DrillChartSchema()` or `StackupTableSchema()`.
fn schema_of(kind: &GeneratedTableKind) -> GeneratedTableSchema {
    match kind {
        GeneratedTableKind::DrillChart { .. } => drill_chart_schema(),
        GeneratedTableKind::StackupTable => stackup_table_schema(),
    }
}

/// Port of the static `formatDrillSpan( OUTPUTFORMATTER* aOut, const DRILL_SPAN& aSpan )`:
/// `"start" "end" [backdrill] [npth]`.
fn format_drill_span(out: &mut dyn OutputFormatter, span: &DrillSpan) -> Result<(), IoError> {
    let start = out.quotew(&Lset::name(span.drill_start_layer()));
    let end = out.quotew(&Lset::name(span.drill_end_layer()));
    out.print(format_args!("{} {}", start, end))?;

    if span.is_backdrill {
        out.print(format_args!(" backdrill"))?;
    }

    if span.is_non_plated_file {
        out.print(format_args!(" npth"))?;
    }

    Ok(())
}

impl PcbWriter<'_> {
    /// Port of `format( const PCB_REFERENCE_IMAGE* aBitmap )`: `(image ..)`.
    pub(crate) fn format_reference_image(&mut self, image: ImageId) -> Result<(), IoError> {
        let Some(bitmap) = self.src.image(image) else {
            return Ok(());
        };

        let ctx = self.ctx;

        // wxCHECK_RET( image != nullptr, "wxImage* is NULL" )
        if !bitmap.has_image() {
            debug_assert!(false, "wxImage* is NULL");
            return Ok(());
        }

        let pos = bitmap.position(&ctx);
        self.out.print(format_args!(
            "(image (at {} {})",
            format_iu(pos.x),
            format_iu(pos.y)
        ))?;

        self.format_layer(bitmap.layer(&ctx), false)?;

        if !bitmap.orientation().is_zero() {
            self.out.print(format_args!(
                "(rotate {})",
                format_angle(bitmap.orientation().as_degrees())
            ))?;
        }

        if bitmap.image_scale() != 1.0 {
            self.out.print(format_args!(
                "(scale {})",
                format_g(bitmap.image_scale(), 6)
            ))?;
        }

        if bitmap.opacity() < 1.0 {
            self.out
                .print(format_args!("(opacity {})", format_g(bitmap.opacity(), 6)))?;
        }

        if bitmap.is_locked(&ctx) {
            format_bool(self.out, "locked", true)?;
        }

        // refImage.GetImage().SaveImageData( ostream ): the original image bytes
        format_stream_data(self.out, &bitmap.image_data())?;

        format_uuid(self.out, &bitmap.base.uuid.as_string())?;
        format_custom_properties(self.out, &bitmap.base.custom_properties)?;
        self.out.print(format_args!(")"))?; // Closes image token.
        Ok(())
    }

    /// Port of `format( const PCB_POINT* aPoint )`: `(point ..)`.
    pub(crate) fn format_point(&mut self, point: PointId) -> Result<(), IoError> {
        let Some(p) = self.src.point(point) else {
            return Ok(());
        };

        let ctx = self.ctx;

        self.out.print(format_args!(
            "(point (at {}) (size {})",
            format_iu_pt(p.lib_pos()),
            format_iu(p.size())
        ))?;

        self.format_layer(p.layer(&ctx), false)?;

        format_uuid(self.out, &p.base.uuid.as_string())?;
        format_custom_properties(self.out, &p.base.custom_properties)?;
        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `format( const PCB_TARGET* aTarget )`: `(target ..)`.
    pub(crate) fn format_target(&mut self, target: TargetId) -> Result<(), IoError> {
        let Some(t) = self.src.target(target) else {
            return Ok(());
        };

        let ctx = self.ctx;

        self.out.print(format_args!(
            "(target {} (at {}) (size {})",
            if t.shape() != 0 { "x" } else { "plus" },
            format_iu_pt(t.position(&ctx)),
            format_iu(t.size())
        ))?;

        if t.width() != 0 {
            self.out
                .print(format_args!("(width {})", format_iu(t.width())))?;
        }

        self.format_layer(t.layer(&ctx), false)?;
        format_uuid(self.out, &t.base.uuid.as_string())?;
        format_custom_properties(self.out, &t.base.custom_properties)?;
        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `format( const PCB_GRID_ITEM* aGridItem )`: `(grid_item ..)`.
    pub(crate) fn format_grid_item(&mut self, grid_item: GridItemId) -> Result<(), IoError> {
        let Some(g) = self.src.grid_item(grid_item) else {
            return Ok(());
        };

        let ctx = self.ctx;
        let polar = g.grid_item_type() == PcbGridType::Polar;

        // Grid type (polar/xy) must be emitted before extent/spacing: those tokens change
        // meaning based on the type (y component = angle for polar, length for cartesian).
        self.out.print(format_args!(
            "(grid_item {} (at {})",
            if polar { "polar" } else { "xy" },
            format_iu_pt(g.position(&ctx))
        ))?;

        if polar {
            // Polar y components are angles; use FormatAngle, not formatInternalUnits.
            // (GetRadiusExtent() / GetRadiusSpacing(): m_extent.x / m_spacing.x)
            self.out.print(format_args!(
                " (extent {} {}) (spacing {} {})",
                format_iu(g.extent().x),
                format_angle(g.phi_extent().as_degrees()),
                format_iu(g.spacing().x),
                format_angle(g.phi_spacing().as_degrees())
            ))?;
        } else {
            self.out.print(format_args!(
                " (extent {}) (spacing {})",
                format_iu_pt(g.extent()),
                format_iu_pt(g.spacing())
            ))?;
        }

        if !g.orientation().is_zero() {
            self.out.print(format_args!(
                " (angle {})",
                format_angle(g.orientation().as_degrees())
            ))?;
        }

        // Priority is always set
        self.out
            .print(format_args!(" (priority {})", g.assigned_priority()))?;

        if g.tick_interval() > 0 {
            self.out
                .print(format_args!(" (tick_interval {})", g.tick_interval()))?;
        }

        let aff = g.affects();
        self.out.print(format_args!(
            " (affects (cursor {}) (routing {}) (placement {}))",
            if aff.cursor { "yes" } else { "no" },
            if aff.routing { "yes" } else { "no" },
            if aff.placement { "yes" } else { "no" }
        ))?;

        if g.is_locked(&ctx) {
            format_bool(self.out, "locked", true)?;
        }

        format_uuid(self.out, &g.base.uuid.as_string())?;
        format_custom_properties(self.out, &g.base.custom_properties)?;
        self.out.print(format_args!(")\n"))?;
        Ok(())
    }

    /// Port of `format( const PCB_BARCODE* aBarcode )`: `(barcode ..)`.
    pub(crate) fn format_barcode(&mut self, barcode: BarcodeId) -> Result<(), IoError> {
        let Some(b) = self.src.barcode(barcode) else {
            return Ok(());
        };

        let ctx = self.ctx;

        self.out.print(format_args!("(barcode"))?;

        if b.is_locked(&ctx) {
            format_bool(self.out, "locked", true)?;
        }

        self.out.print(format_args!(
            "(at {} {})",
            format_iu_pt(b.position(&ctx)),
            format_angle(b.angle(&ctx).as_degrees())
        ))?;

        self.format_layer(b.layer(&ctx), false)?;

        self.out.print(format_args!(
            "(size {} {})",
            format_iu(b.width),
            format_iu(b.height)
        ))?;

        // GetText(): m_text.GetText()
        let text = self.out.quotew(b.text.text.text());
        self.out.print(format_args!("(text {})", text))?;

        // GetTextSize(): m_text.GetTextHeight(), the virtual GetTextSize().y (board frame).
        self.out.print(format_args!(
            "(text_height {})",
            format_iu(b.text_size(&ctx))
        ))?;

        let type_str = match b.kind {
            BarcodeT::Code39 => "code39",
            BarcodeT::Code128 => "code128",
            BarcodeT::DataMatrix => "datamatrix",
            BarcodeT::QrCode => "qr",
            BarcodeT::MicroQrCode => "microqr",
        };

        self.out.print(format_args!("(type {})", type_str))?;

        if b.kind == BarcodeT::QrCode || b.kind == BarcodeT::MicroQrCode {
            let ecc_str = match b.error_correction {
                BarcodeEccT::L => "L",
                BarcodeEccT::M => "M",
                BarcodeEccT::Q => "Q",
                BarcodeEccT::H => "H",
            };

            self.out.print(format_args!("(ecc_level {})", ecc_str))?;
        }

        // GetShowText(): m_text.IsVisible()
        format_bool(self.out, "hide", !b.text.text.is_visible())?;
        format_bool(self.out, "knockout", b.is_knockout())?;

        if b.margin.x != 0 || b.margin.y != 0 {
            self.out.print(format_args!(
                "(margins {} {})",
                format_iu(b.margin.x),
                format_iu(b.margin.y)
            ))?;
        }

        format_uuid(self.out, &b.base.uuid.as_string())?;

        format_custom_properties(self.out, &b.base.custom_properties)?;
        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `formatTableData( const PCB_TABLE* aTable )`: geometry and cells with no
    /// identity, so an enclosing form owns uuid/layer/locked.
    pub(crate) fn format_table_data(&mut self, table: TableId) -> Result<(), IoError> {
        let Some(t) = self.src.table(table) else {
            return Ok(());
        };

        // Carries geometry and cells only. Identity belongs to whatever encloses this, or a
        // drill chart would write its uuid and layer twice and the second copy would win on load
        self.out
            .print(format_args!("(column_count {})", t.col_count()))?;

        self.out.print(format_args!("(border"))?;
        format_bool(self.out, "external", t.stroke_external)?;
        format_bool(self.out, "header", t.stroke_header_separator)?;

        if t.stroke_external || t.stroke_header_separator {
            t.border_stroke.format(self.out, &PCB_IU_SCALE)?;
        }

        self.out.print(format_args!(")"))?; // Close `border` token.

        self.out.print(format_args!("(separators"))?;
        format_bool(self.out, "rows", t.stroke_rows)?;
        format_bool(self.out, "cols", t.stroke_columns)?;

        if t.stroke_rows || t.stroke_columns {
            t.separators_stroke.format(self.out, &PCB_IU_SCALE)?;
        }

        self.out.print(format_args!(")"))?; // Close `separators` token.

        self.out.print(format_args!("(column_widths"))?;

        for col in 0..t.col_count() {
            self.out
                .print(format_args!(" {}", format_iu(t.col_width(col))))?;
        }

        self.out.print(format_args!(")"))?;

        self.out.print(format_args!("(row_heights"))?;

        for row in 0..t.row_count() {
            self.out
                .print(format_args!(" {}", format_iu(t.row_height(row))))?;
        }

        self.out.print(format_args!(")"))?;

        self.out.print(format_args!("(cells"))?;

        for &cell_id in &t.cells {
            if let Some(cell) = self.src.tablecell(cell_id) {
                self.format_textbox_with(
                    cell,
                    &cell.textbox,
                    Some((cell.col_span(), cell.row_span())),
                )?;
            }
        }

        self.out.print(format_args!(")"))?; // Close `cells` token.

        format_custom_properties(self.out, &t.base.custom_properties)?;
        Ok(())
    }

    /// Port of `format( const PCB_DRILL_MAP* aMap )`: `(drill_map ..)`.
    pub(crate) fn format_drill_map(&mut self, drill_map: DrillMapId) -> Result<(), IoError> {
        let Some(map) = self.src.drill_map(drill_map) else {
            return Ok(());
        };

        let ctx = self.ctx;

        self.out.print(format_args!("(drill_map "))?;

        format_uuid(self.out, &map.base.uuid.as_string())?;

        if map.is_locked(&ctx) {
            format_bool(self.out, "locked", true)?;
        }

        self.format_layer(map.layer(&ctx), false)?;

        // An offset applied to every mark, not a place of its own. Zero puts the marks on their
        // holes
        self.out
            .print(format_args!("(offset {})", format_iu_pt(map.offset())))?;
        self.out
            .print(format_args!("(size {})", format_iu(map.symbol_size())))?;

        if map.all_spans() {
            self.out.print(format_args!("(span all)"))?;
        } else {
            self.out.print(format_args!("(span "))?;
            format_drill_span(self.out, map.span())?;
            self.out.print(format_args!(")"))?;
        }

        format_bool(self.out, "outline_slots", map.outline_slots())?;
        format_bool(self.out, "guide_cross", map.guide_cross())?;
        format_custom_properties(self.out, &map.base.custom_properties)?;

        self.out.print(format_args!(")"))?; // Close `drill_map` token.
        Ok(())
    }

    /// Port of `format( const PCB_GENERATED_TABLE* aTable )`: `(drill_chart ..)` /
    /// `(stackup_table ..)`.
    pub(crate) fn format_generated_table(&mut self, table: TableId) -> Result<(), IoError> {
        let Some(t) = self.src.table(table) else {
            return Ok(());
        };

        let ctx = self.ctx;

        let Some(generated) = t.generated.as_deref() else {
            // wxFAIL_MSG( "Unhandled generated table type " + aTable->GetClass() )
            debug_assert!(false, "Unhandled generated table type {}", t.table_class());
            return Ok(());
        };

        match generated.kind {
            GeneratedTableKind::DrillChart { .. } => {
                self.out.print(format_args!("(drill_chart "))?;
            }
            GeneratedTableKind::StackupTable => {
                self.out.print(format_args!("(stackup_table "))?;
            }
        }

        format_uuid(self.out, &t.base.uuid.as_string())?;

        if BoardItem::is_locked(t, &ctx) {
            format_bool(self.out, "locked", true)?;
        }

        self.format_layer(BoardItem::layer(t, &ctx), false)?;

        // Anything the reader would arrive at on its own is left out. A table nobody has
        // reconfigured is a handful of bytes rather than a page of them
        let schema = schema_of(&generated.kind);

        if generated.units != schema.default_units {
            self.out.print(format_args!(
                "(units {})",
                generated_table_units_token(generated.units)
            ))?;
        }

        if generated.precision != schema.default_precision {
            self.out
                .print(format_args!("(precision {})", generated.precision))?;
        }

        for col in generated.all_columns() {
            let def = schema.find(col.id);

            self.out
                .print(format_args!("(column (id {})", schema.token(col.id)))?;

            // Compared against the def's own literal, never DefaultColumn()'s translated text,
            // so a heading written under one UI language reads back unchanged under another
            if def.is_none_or(|d| col.heading != d.heading) {
                let heading = self.out.quotew(&col.heading);
                self.out.print(format_args!("(name {})", heading))?;
            }

            if def.is_none_or(|d| col.align != d.align) {
                self.out.print(format_args!(
                    "(justify {})",
                    generated_table_align_token(col.align)
                ))?;
            }

            if col.width > 0 {
                self.out
                    .print(format_args!("(width {})", format_iu(col.width)))?;
            }

            if !col.visible {
                format_bool(self.out, "hide", true)?;
            }

            self.out.print(format_args!(")"))?;
        }

        if !generated.row_keys.is_empty() {
            // What each row reports, so a rebuild after the board has changed can hand a row's
            // formatting to the row that still reports the same thing
            self.out.print(format_args!("(row_keys"))?;

            for (row, key) in &generated.row_keys {
                let key = self.out.quotew(key);
                self.out.print(format_args!("(key {} {})", row, key))?;
            }

            self.out.print(format_args!(")"))?;
        }

        self.format_generated_table_extras(table)?;

        // The geometry and cells a generated table shares with any other table, written the
        // same way
        self.format_table_data(table)?;

        self.out.print(format_args!(")"))?; // Close the table's kind token
        Ok(())
    }

    /// Port of `formatGeneratedTableExtras( const PCB_GENERATED_TABLE* )`: what only one kind
    /// of generated table carries, written after the shared settings.
    pub(crate) fn format_generated_table_extras(&mut self, table: TableId) -> Result<(), IoError> {
        let Some(t) = self.src.table(table) else {
            return Ok(());
        };

        let Some(GeneratedTableKind::DrillChart {
            filter,
            show_totals,
            row_shapes,
            symbol_column,
        }) = t.generated.as_deref().map(|g| &g.kind)
        else {
            return Ok(());
        };

        // DRILL_CHART_TEMPLATE::MakeDefault().GetShowTotals(): the template constructor's
        // m_showTotals( true ) (MakeDefault only sets the name and the columns)
        let default_show_totals = true;

        if *filter != DrillChartFilter::default() {
            self.out.print(format_args!("(filter"))?;
            format_bool(self.out, "plated", filter.plated)?;
            format_bool(self.out, "npth", filter.non_plated)?;
            format_bool(self.out, "vias", filter.vias)?;
            format_bool(self.out, "slots", filter.slots)?;
            format_bool(self.out, "backdrill", filter.backdrills)?;
            format_bool(self.out, "castellated", filter.castellated)?;
            self.out.print(format_args!(")"))?;
        }

        if *show_totals != default_show_totals {
            format_bool(self.out, "totals", *show_totals)?;
        }

        if *symbol_column >= 0 && !row_shapes.is_empty() {
            // Generated payload, like the cell text. A shape mark has no text of its own, so
            // without this the symbol column comes back blank
            self.out
                .print(format_args!("(row_shapes (column {})", symbol_column))?;

            for (row, shape_index) in row_shapes {
                self.out
                    .print(format_args!("(shape {} {})", row, shape_index))?;
            }

            self.out.print(format_args!(")"))?;
        }

        Ok(())
    }

    /// Port of `format( const PCB_TABLE* aTable )`: `(table ..)`.
    pub(crate) fn format_table(&mut self, table: TableId) -> Result<(), IoError> {
        let Some(t) = self.src.table(table) else {
            return Ok(());
        };

        let ctx = self.ctx;

        self.out.print(format_args!("(table "))?;

        format_uuid(self.out, &t.base.uuid.as_string())?;

        if BoardItem::is_locked(t, &ctx) {
            format_bool(self.out, "locked", true)?;
        }

        self.format_layer(BoardItem::layer(t, &ctx), false)?;

        self.format_table_data(table)?;

        self.out.print(format_args!(")"))?; // Close `table` token.
        Ok(())
    }
}
