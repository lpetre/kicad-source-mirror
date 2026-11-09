#!/bin/sh
# Regenerates m23_generator_properties.txt (package M23, PcbGenerator::normalize_properties):
# every (generated ...) block of the listed corpus boards before and after the reference
# `kicad-cli pcb upgrade --force` (PCB_GENERATOR::SetProperties + GetProperties + writer).
# Members and templates are stripped. Run from <kicad>/rust; needs the oracle (docs/ORACLE.md).
set -e
KICAD=$(cd .. && pwd)
OUT=crates/kicad-pcb/tests/golden/m23_generator_properties.txt
TMP=$(mktemp -d)
mkdir -p "$TMP/in" "$TMP/out" "$TMP/home"
: > "$OUT"
for f in \
    qa/data/pcbnew/drc_via_stack/StackNotTilingItsSpanReported.kicad_pcb \
    qa/data/pcbnew/via_stacks.kicad_pcb \
    qa/data/pcbnew/tuning_generators_load_save.kicad_pcb \
    qa/data/pcbnew/tuning_generators_load_save_v20231212.kicad_pcb \
    qa/data/pcbnew/diff_pair_uncoupled_tuning_drc.kicad_pcb \
    qa/data/downgrade/regressions/via_generators/via_generators.kicad_pcb \
    qa/data/pcbnew/stitch_fp_rulearea.kicad_pcb \
    qa/data/pcbnew/issue25303.kicad_pcb \
    qa/data/pcbnew/issue25265.kicad_pcb \
    qa/data/pcbnew/issue17971/issue17971.kicad_pcb \
    qa/data/pcbnew/issue23550/issue23550.kicad_pcb \
    qa/data/pcbnew/issue18045/test_proj.kicad_pcb \
    qa/data/pcbnew/drc_skew/drc_skew.kicad_pcb \
    qa/data/pcbnew/issue22536.kicad_pcb \
    qa/data/pcbnew/multichannel_nested.kicad_pcb
do
    b=$(basename "$f")
    cp "$KICAD/$f" "$TMP/in/$b"
    cp "$KICAD/$f" "$TMP/out/$b"
    (cd "$TMP/out" && KICAD_ORACLE_HOME="$TMP/home" /home/user/kicad-oracle/bin/kicad-cli pcb upgrade --force "$b" >/dev/null 2>&1)
    python3 -I crates/kicad-pcb/tests/golden/m23_generator_properties.py "$b" "$TMP/in/$b" "$TMP/out/$b" >> "$OUT"
done
rm -rf "$TMP"
