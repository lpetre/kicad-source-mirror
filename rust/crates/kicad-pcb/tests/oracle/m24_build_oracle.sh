#!/bin/bash
# Builds and runs the M24 oracle (reference images, embedded files) against the reference
# build and writes the goldens tests/golden/m24_images.txt and tests/golden/m24_embed.txt.
#
#   ORACLE_BUILD  reference build (default /home/user/kicad-oracle/build)
#   OUT_DIR       scratch dir     (default <rust>/target/m24-oracle)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CRATE="$(cd "$HERE/../.." && pwd)"
KICAD_SRC="$(cd "$CRATE/../../.." && pwd)"
ORACLE_BUILD="${ORACLE_BUILD:-/home/user/kicad-oracle/build}"
OUT_DIR="${OUT_DIR:-$KICAD_SRC/rust/target/m24-oracle}"
mkdir -p "$OUT_DIR"

# Compile flags of common/reference_image.cpp (without the precompiled header).
FLAGS_FILE="$OUT_DIR/cflags.txt"
python3 - "$ORACLE_BUILD" "$FLAGS_FILE" <<'PY'
import json, shlex, sys
build, out = sys.argv[1], sys.argv[2]
for e in json.load(open(build + "/compile_commands.json")):
    if e["output"].endswith("common.dir/reference_image.cpp.o"):
        args = shlex.split(e["command"])[1:]
        keep, skip = [], False
        for a in args:
            if skip: skip = False; continue
            if a in ("-o", "-c", "-include"): skip = True; continue
            if a.endswith(".cpp") or a.startswith("-Winvalid-pch") or a.startswith("-fvisibility"):
                continue
            keep.append(a)
        open(out, "w").write(" ".join(shlex.quote(a) for a in keep))
        break
else:
    sys.exit("reference_image.cpp.o not found")
PY
eval "CFLAGS=( $(cat "$FLAGS_FILE") )"
CFLAGS+=( -w )

for src in "$HERE/m24_oracle.cpp" "$KICAD_SRC/common/reference_image.cpp" "$KICAD_SRC/common/bitmap_base.cpp"; do
    nice -n 15 c++ "${CFLAGS[@]}" -c "$src" -o "$OUT_DIR/$(basename "$src" .cpp).o"
done

nice -n 15 c++ -o "$OUT_DIR/m24_oracle" "$OUT_DIR"/m24_oracle.o "$OUT_DIR"/reference_image.o \
    "$OUT_DIR"/bitmap_base.o "$ORACLE_BUILD/common/libkicommon.so" \
    "$ORACLE_BUILD/libs/kimath/libkimath.a" "$ORACLE_BUILD/libs/core/libcore.a" \
    -Wl,-rpath,"$ORACLE_BUILD/common" $(wx-config --libs core,base) -lz -lpthread

cd "$KICAD_SRC"
BOARDS=$(grep -rlE --include=*.kicad_pcb '\(image\s' qa/data demos | sort)
"$OUT_DIR/m24_oracle" images "$KICAD_SRC" $BOARDS > "$CRATE/tests/golden/m24_images.txt"
EMBED=$(grep -rl '(embedded_files' qa/data demos | sort)
"$OUT_DIR/m24_oracle" embed "$KICAD_SRC" $EMBED > "$CRATE/tests/golden/m24_embed.txt"
wc -l "$CRATE/tests/golden/m24_images.txt" "$CRATE/tests/golden/m24_embed.txt"
