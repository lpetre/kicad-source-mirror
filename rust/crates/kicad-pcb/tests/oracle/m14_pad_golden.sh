#!/bin/sh
# This program source code file is part of KiCad, a free EDA CAD application.
#
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU General Public License as published by the
# Free Software Foundation, either version 3 of the License, or (at your
# option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
# General Public License for more details.
#
# You should have received a copy of the GNU General Public License along
# with this program.  If not, see <http://www.gnu.org/licenses/>.
#
# Package M14 (pad model): regenerates tests/golden/m14_pad_frames.txt from the reference
# pcb-dump (rust/docs/ORACLE.md) on a scratch copy of tests/oracle/m14_pad_frames.kicad_pcb
# (rotated / flipped / non-uniformly scaled footprints, NORMAL / FRONT_INNER_BACK / CUSTOM
# padstacks). The Rust replay is `pad::derive::tests::oracle_pad_frames`.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
SCRATCH=$(mktemp -d)
cp "$HERE/m14_pad_frames.kicad_pcb" "$SCRATCH/m14_pad_frames.kicad_pcb"
KICAD_ORACLE_HOME="$SCRATCH/home" /home/user/kicad-oracle/bin/pcb-dump board \
    --sections item,pad,footprint "$SCRATCH/m14_pad_frames.kicad_pcb" > "$SCRATCH/ref.txt"
# Keep the member and derived fields the M14 replay reads (no shapes / polygons: M15).
grep -E '^(footprint\|[^|]*\|(transform\.|layer=|flipped=|local_)|item\|[^|]*\|(layer=|net=|netname=|fp_relative_position=|locked=|on_copper=|has_hole=|has_drilled_hole=)|pad\|[^|]*\|(number|pin_type|attribute|property|position|library_position|orientation|fp_relative_orientation|primitive_lib_scale|flipped|aperture|npth_no_copper|no_connect|free|can_have_number|principal_layer|drill_size|local_zone_connection|zone_connection_overrides|clearance_overrides|local_thermal_spoke_width|local_spoke_width_override|thermal_spoke_angle|thermal_gap|local_thermal_gap|unconnected_layer_mode|solder_mask_expansion\..*|solder_paste_margin\..*|padstack\.(mode|layers|drill\.(size|shape|start|end)|front_outer\.solder_[a-z_]*|back_outer\.solder_[a-z_]*|copper\.[^.]+\.[^.]+\.(shape|anchor_shape|size|offset|round_rect_radius_ratio|chamfered_rect_ratio|chamfered_rect_positions|trapezoid_delta_size|zone_connection|thermal_spoke_width|thermal_spoke_angle|thermal_gap|clearance))|layer\.[^.]+\.[^.]+\.(shape|size|offset|delta|round_rect_corner_radius)|on_layer\..*|flash\..*|flash_permitted\..*)=)' \
    "$SCRATCH/ref.txt" > "$HERE/../golden/m14_pad_frames.txt"
rm -rf "$SCRATCH"
