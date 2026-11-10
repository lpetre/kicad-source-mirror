#!/usr/bin/env bash
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# WASM-API-1: golden of the router read API (tests/board_api_read.rs). Runs the reference
# `pcb-dump board --loader --hash` (the kicad-cli load path) on the boards below, one at a
# time, and keeps the hashed lines of the fields the read API returns. No C++ build involved.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"          # KiCad source root
OUT="$ROOT/rust/crates/kicad-session/tests/golden/board_api_read.txt"
DUMP="${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/board_api_golden.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

BOARDS=(
  demos/ecc83/ecc83-pp.kicad_pcb
  demos/ecc83/ecc83-pp_v2.kicad_pcb
  demos/constraints/constraints.kicad_pcb
  demos/microwave/microwave.kicad_pcb
  qa/data/pcbnew/via_stacks.kicad_pcb
  qa/data/pcbnew/issue18142.kicad_pcb
  qa/data/pcbnew/via_stack_uvia_rule.kicad_pcb
  qa/data/pcbnew/via_stack_track_width.kicad_pcb
  qa/data/pcbnew/padstacks.kicad_pcb
  qa/data/pcbnew/odbpp/custom_pad_mask_split.kicad_pcb
  qa/data/pcbnew/odbpp/pad_margin_shapes.kicad_pcb
  qa/data/pcbnew/odbpp/npth_pressfit_slot.kicad_pcb
  qa/data/pcbnew/keepout_disallow/keepout_disallow.kicad_pcb
  qa/data/pcbnew/keepout_no_clearance/keepout_no_clearance.kicad_pcb
  qa/data/pcbnew/stitch_fp_rulearea.kicad_pcb
  qa/data/pcbnew/notched_zones.kicad_pcb
  qa/data/pcbnew/intersectingzones.kicad_pcb
  qa/data/pcbnew/tracks_arcs_vias.kicad_pcb
  qa/data/pcbnew/teardrop_spike.kicad_pcb
  qa/data/pcbnew/two_segment_teardrop.kicad_pcb
  qa/data/pcbnew/net_tie_drc.kicad_pcb
  qa/data/pcbnew/severities.kicad_pcb
  qa/data/pcbnew/connection_width_rules.kicad_pcb
  qa/data/pcbnew/multinetclasses_drc.kicad_pcb
  qa/data/pcbnew/diff_pair_uncoupled_tuning_drc.kicad_pcb
  qa/data/pcbnew/component_classes_drc.kicad_pcb
  qa/data/pcbnew/issue16566.kicad_pcb
  qa/data/pcbnew/issue7567.kicad_pcb
  qa/data/pcbnew/issue10697.kicad_pcb
  qa/data/pcbnew/issue22010/issue22010.kicad_pcb
  qa/data/pcbnew/length_calculations.kicad_pcb
  qa/data/pcbnew/drc_via_stack/MicroviaStackDepthRuleIsHonoured.kicad_pcb
)

# SECTION|KEY|FIELD lines the read API covers (extended regex on "SECTION|KEY|FIELD=").
KEEP='^(board\|board\|(copper_layer_count|enabled_layer_names|layer\.[A-Za-z0-9]+\.Cu\.(name|type)|bbox|outline_inferred(\.ok)?)='
KEEP+='|item\|[^|]*\|(net|locked)='
KEEP+='|pad\|[^|]*\|(number|attribute|position|orientation|drill_size|drill_shape|layer\.[^.]+\.[^.]+\.(shape|size|offset|round_rect_corner_radius|chamfer_ratio|chamfer_positions)|layer\.[^.]+\.(shape|size|offset|round_rect_corner_radius|chamfer_ratio|chamfer_positions)|flash\.[^=]*|effective_polygon\.[^=]*)='
KEEP+='|track\|[^|]*\|(start|end|width|mid|via_type|top_layer|bottom_layer|drill_value|is_free|width\.[^=]*)='
KEEP+='|footprint\|[^|]*\|(reference|value|position|orientation|side|courtyard\.[FB])='
KEEP+='|bbox\|[^|]*\|bbox='
KEEP+='|zone\|[^|]*\|(name|priority|rule_area|keepout\.[a-z_]+|teardrop_area|local_clearance|min_thickness|pad_connection|island_removal_mode|outline|filled\.[^=]*)=)'

: > "$OUT.tmp"
for b in "${BOARDS[@]}"; do
  src="$ROOT/$b"
  d="$SCRATCH/$(echo "$b" | tr / _)"
  mkdir -p "$d"
  cp "$src" "$d/"
  for ext in kicad_pro kicad_dru; do
    [ -f "${src%.kicad_pcb}.$ext" ] && cp "${src%.kicad_pcb}.$ext" "$d/"
  done
  echo "file|$b|" >> "$OUT.tmp"
  nice -n 10 "$DUMP" board --loader --hash --sections board,item,bbox,track,pad,footprint,zone \
      "$d/$(basename "$b")" 2>/dev/null | grep -E "$KEEP" >> "$OUT.tmp" || true
  rm -rf "$d"
done
# bbox lines only for footprints (the read API returns no other bounding box)
awk -F'|' 'NR==FNR { if ($1 == "footprint") fp[$2] = 1; next }
           $1 != "bbox" || ($2 in fp)' "$OUT.tmp" "$OUT.tmp" > "$OUT"
rm -f "$OUT.tmp"
wc -lc "$OUT"
