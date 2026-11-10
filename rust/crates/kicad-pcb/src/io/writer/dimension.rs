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
//! PCBIO-10: `format( const PCB_DIMENSION_BASE* )` (W:1067-1180).
//!
//! The `dynamic_cast`s of the C++ become the `DimensionKind` of the dimension; note that an
//! orthogonal dimension is also a `PCB_DIM_ALIGNED` (height, arrow direction and extension
//! height are written for both).

use kicad_sexpr::{IoError, format_bool, format_custom_properties, format_uuid};

use super::{PcbWriter, format_iu};
use crate::ids::DimensionId;
use crate::pcb_dimension::{DimArrowDirection, DimensionKind};
use crate::traits::BoardItem;

impl PcbWriter<'_> {
    /// Port of `format( const PCB_DIMENSION_BASE* aDimension )`: `(dimension ..)`.
    pub(crate) fn format_dimension(&mut self, dimension: DimensionId) -> Result<(), IoError> {
        let Some(dim) = self.src.dimension(dimension) else {
            return Ok(());
        };

        let ctx = self.ctx;

        let (aligned, ortho, center, radial, leader) = match &dim.kind {
            DimensionKind::Aligned(a) => (Some(a), None, false, None, None),
            DimensionKind::Orthogonal {
                aligned,
                orientation,
            } => (Some(aligned), Some(*orientation), false, None, None),
            DimensionKind::Center => (None, None, true, None, None),
            DimensionKind::Radial { leader_length } => {
                (None, None, false, Some(*leader_length), None)
            }
            DimensionKind::Leader { text_border } => (None, None, false, None, Some(*text_border)),
        };

        self.out.print(format_args!("(dimension"))?;

        // must be tested before aligned, because ortho is derived from aligned
        // and aligned is not null
        if ortho.is_some() {
            self.out.print(format_args!("(type orthogonal)"))?;
        } else if aligned.is_some() {
            self.out.print(format_args!("(type aligned)"))?;
        } else if leader.is_some() {
            self.out.print(format_args!("(type leader)"))?;
        } else if center {
            self.out.print(format_args!("(type center)"))?;
        } else if radial.is_some() {
            self.out.print(format_args!("(type radial)"))?;
        }

        if dim.is_locked(&ctx) {
            format_bool(self.out, "locked", dim.is_locked(&ctx))?;
        }

        self.format_layer(dim.layer(&ctx), false)?;

        format_uuid(self.out, &dim.base().uuid.as_string())?;

        let start = dim.start(&ctx);
        let end = dim.end(&ctx);
        self.out.print(format_args!(
            "(pts (xy {} {}) (xy {} {}))",
            format_iu(start.x),
            format_iu(start.y),
            format_iu(end.x),
            format_iu(end.y)
        ))?;

        if let Some(aligned) = aligned {
            self.out
                .print(format_args!("(height {})", format_iu(aligned.height)))?;
        }

        if let Some(leader_length) = radial {
            self.out
                .print(format_args!("(leader_length {})", format_iu(leader_length)))?;
        }

        if let Some(orientation) = ortho {
            self.out
                .print(format_args!("(orientation {})", orientation as i32))?;
        }

        if !center {
            let prefix = self.out.quotew(&dim.prefix);
            let suffix = self.out.quotew(&dim.suffix);
            self.out.print(format_args!(
                "(format (prefix {}) (suffix {}) (units {}) (units_format {}) (precision {})",
                prefix,
                suffix,
                dim.units_mode() as i32,
                dim.units_format as i32,
                dim.precision as i32
            ))?;

            if dim.override_text_enabled {
                let value = self.out.quotew(dim.override_text());
                self.out
                    .print(format_args!("(override_value {})", value))?;
            }

            if dim.suppress_zeroes {
                format_bool(self.out, "suppress_zeroes", true)?;
            }

            self.out.print(format_args!(")"))?;
        }

        self.out.print(format_args!(
            "(style (thickness {}) (arrow_length {}) (text_position_mode {})",
            format_iu(dim.line_thickness),
            format_iu(dim.arrow_length),
            dim.text_position as i32
        ))?;

        if ortho.is_some() || aligned.is_some() {
            match dim.arrow_direction {
                DimArrowDirection::Outward => {
                    self.out.print(format_args!("(arrow_direction outward)"))?;
                }
                DimArrowDirection::Inward => {
                    self.out.print(format_args!("(arrow_direction inward)"))?;
                }
                // No default, handle all cases
            }
        }

        if let Some(aligned) = aligned {
            self.out.print(format_args!(
                "(extension_height {})",
                format_iu(aligned.extension_height)
            ))?;
        }

        if let Some(text_border) = leader {
            self.out
                .print(format_args!("(text_frame {})", text_border as i32))?;
        }

        self.out.print(format_args!(
            "(extension_offset {})",
            format_iu(dim.extension_offset)
        ))?;

        if dim.keep_text_aligned {
            format_bool(self.out, "keep_text_aligned", true)?;
        }

        self.out.print(format_args!(")"))?;

        // Write dimension text after all other options to be sure the
        // text options are known when reading the file
        if !center {
            self.format_pcb_text_with(dim, &dim.text, None, true)?;
        }

        format_custom_properties(self.out, &dim.base().custom_properties)?;
        self.out.print(format_args!(")"))?;

        Ok(())
    }
}
