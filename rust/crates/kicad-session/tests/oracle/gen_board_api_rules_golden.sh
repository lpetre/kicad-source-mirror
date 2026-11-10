#!/usr/bin/env bash
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# WASM-API-1: golden of Session::constraint (tests/board_api_rules.rs): a deterministic sample
# of the reference `pcb-dump board --sections evalrules` lines (DRC_ENGINE::EvalRules on
# single items and item pairs) of boards with custom rules. No C++ build involved.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"
OUT="$ROOT/rust/crates/kicad-session/tests/golden/board_api_rules.txt"
DUMP="${PCB_DUMP:-/home/user/kicad-oracle/bin/pcb-dump}"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/board_api_rules.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

BOARDS=(
  qa/data/pcbnew/connection_width_rules.kicad_pcb
  qa/data/pcbnew/multinetclasses_drc.kicad_pcb
  qa/data/pcbnew/via_stack_uvia_rule.kicad_pcb
  qa/data/pcbnew/via_stack_track_width.kicad_pcb
  qa/data/pcbnew/diff_pair_uncoupled_tuning_drc.kicad_pcb
  qa/data/pcbnew/issue16566.kicad_pcb
  qa/data/pcbnew/issue7567.kicad_pcb
  qa/data/pcbnew/keepout_no_clearance/keepout_no_clearance.kicad_pcb
  demos/constraints/constraints.kicad_pcb
)

: > "$OUT"
for b in "${BOARDS[@]}"; do
  src="$ROOT/$b"
  d="$SCRATCH/$(echo "$b" | tr / _)"
  mkdir -p "$d"
  cp "$src" "$d/"
  for ext in kicad_pro kicad_dru; do
    [ -f "${src%.kicad_pcb}.$ext" ] && cp "${src%.kicad_pcb}.$ext" "$d/"
  done
  echo "file|$b|" >> "$OUT"
  # Constraint lines (not own@) of tracks, arcs, vias, pads and their pairs; every 7th line.
  nice -n 10 "$DUMP" board --sections evalrules "$d/$(basename "$b")" 2>/dev/null \
    | grep -E '^evalrules\|(track|arc|via|pad|pair_[a-z]+):[0-9a-f-]+(:[0-9a-f-]+)?\|c[0-9]+@' \
    | awk 'NR % 7 == 1' >> "$OUT" || true
  rm -rf "$d"
done
wc -lc "$OUT"
