#!/bin/sh
# Build the oracle harness of the kicad-sexpr crate (see oracle.cpp) from the original KiCad
# C++ sources.  Needs a C++20 compiler, CMake and wxWidgets (wx-config, base library only).
#
# usage: build.sh [output directory]      (default: <kicad>/rust/target/kicad-sexpr-oracle)
#
# The binary is written to <output directory>/oracle.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/kicad-sexpr-oracle}
GEN=$OUT/gen

mkdir -p "$GEN"

# Run the real lexer generator for every keywords file kicad-sexpr ships a token enum for.
make_lexer() {
    # make_lexer <keywords file> <output base name> <enum namespace>
    cmake -Denum="$3" \
          -DinputFile="$KICAD/$1" \
          -DoutHeaderFile="$GEN/$2_lexer.h" \
          -DoutCppFile="$GEN/$2_keywords.cpp" \
          -P "$KICAD/cmake/BuildSteps/TokenList2DsnLexer.cmake"
}

make_lexer common/pcb.keywords                              pcb                 PCB_KEYS_T
make_lexer common/pcb_plot_params.keywords                  pcb_plot_params     PCBPLOTPARAMS_T
make_lexer common/stroke_params.keywords                    stroke_params       STROKEPARAMS_T
make_lexer common/embedded_files.keywords                   embedded_files      EMBEDDED_FILES_T
make_lexer common/drc_rules.keywords                        drc_rules           DRCRULE_T
make_lexer common/lib_table.keywords                        lib_table           LIB_TABLE_T
make_lexer common/drawing_sheet/drawing_sheet.keywords      drawing_sheet       DRAWINGSHEET_T
make_lexer common/netlist.keywords                          netlist             NL_T
make_lexer common/template_fieldnames.keywords              template_fieldnames TFIELD_T
make_lexer pcbnew/specctra_import_export/specctra.keywords  specctra            DSN

# -fno-access-control: the harness reads private keyword tables and protected lexer members.
# No -DDEBUG: IO_ERROR::What() is the release build text.
CXX=${CXX:-g++}
CXXFLAGS="-std=c++20 -O1 -w -fno-access-control -DFMT_HEADER_ONLY $(wx-config --cxxflags base)"
INCLUDES="-I$HERE/stubs -I$GEN -I$KICAD/include -I$KICAD/libs/core/include \
          -I$KICAD/libs/sexpr/include -I$KICAD/thirdparty/fmt/include \
          -I$KICAD/thirdparty/fast_float/include"

OBJS=""

compile() {
    obj=$OUT/$(basename "$1" .cpp).o
    OBJS="$OBJS $obj"

    if [ ! -f "$obj" ] || [ "$1" -nt "$obj" ]; then
        echo "compiling $1"
        # shellcheck disable=SC2086
        $CXX $CXXFLAGS $INCLUDES -c "$1" -o "$obj"
    fi
}

compile "$HERE/oracle.cpp"
compile "$KICAD/common/dsnlexer.cpp"
compile "$KICAD/common/richio.cpp"
compile "$KICAD/common/exceptions.cpp"
compile "$KICAD/common/io/kicad/kicad_io_utils.cpp"
compile "$KICAD/libs/sexpr/sexpr.cpp"
compile "$KICAD/libs/sexpr/sexpr_parser.cpp"

for f in "$GEN"/*_keywords.cpp; do
    compile "$f"
done

# shellcheck disable=SC2086
$CXX $OBJS $(wx-config --libs base) -o "$OUT/oracle"
echo "built $OUT/oracle"
