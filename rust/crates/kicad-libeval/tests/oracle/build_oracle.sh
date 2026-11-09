#!/bin/sh
# Builds the C++ reference drivers used to (re)generate tests/golden/*.txt.
#
# Requirements: a C++20 compiler, wxWidgets 3.2 development files (wx-config) and harfbuzz.
# Nothing is written into the source tree: everything goes to the directory given as $1
# (default: <kicad>/rust/target/libeval-oracle).
#
#   tests/oracle/build_oracle.sh [outdir]
#   <outdir>/oracle_libeval < tests/golden/libeval_cases.txt   > tests/golden/libeval_golden.txt
#   <outdir>/oracle_numeric < tests/golden/numeric_cases.txt   > tests/golden/numeric_golden.txt
#
# The parser tables embedded in src/libeval_compiler/grammar.rs and
# src/numeric_evaluator/grammar.rs come from the same Lemon run (see tests/oracle/README.md).
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
OUT=${1:-$KICAD/rust/target/libeval-oracle}

mkdir -p "$OUT/gen/libeval_compiler" "$OUT/gen/libeval"
cd "$OUT"

# 1. Lemon, exactly as the KiCad build runs it (cmake/BuildSteps/LemonParserGenerator.cmake).
cc -O1 -w -o lemon "$KICAD/thirdparty/lemon/lemon.c"
./lemon -T"$KICAD/thirdparty/lemon/lempar.c" -d"$OUT/gen/libeval_compiler" -q \
        "$KICAD/common/libeval_compiler/grammar.lemon"
./lemon -T"$KICAD/thirdparty/lemon/lempar.c" -d"$OUT/gen/libeval" -q \
        "$KICAD/common/libeval/grammar.lemon"

# 2. A config.h is needed by a few KiCad headers; an empty one is enough for these sources.
mkdir -p "$OUT/cfg"
[ -f "$OUT/cfg/config.h" ] || : > "$OUT/cfg/config.h"
[ -f "$OUT/cfg/kicad_build_version.h" ] || : > "$OUT/cfg/kicad_build_version.h"

INC="$(wx-config --cxxflags) -I$KICAD/include -I$KICAD/libs/core/include \
     -I$KICAD/libs/kimath/include -I$KICAD/libs/kiplatform/include -I$OUT/cfg -I$OUT/gen \
     -I$KICAD/thirdparty/fmt/include -I$KICAD/thirdparty/magic_enum \
     -I$KICAD/thirdparty/nlohmann_json -I$KICAD/thirdparty"
CXX="${CXX:-c++} -std=c++20 -O1 -w $INC"

$CXX -c "$KICAD/common/libeval_compiler/libeval_compiler.cpp" -o libeval_compiler.o
$CXX -c "$KICAD/common/libeval/numeric_evaluator.cpp" -o numeric_evaluator.o
$CXX -c "$KICAD/common/string_utils.cpp" -o string_utils.o
$CXX -c "$KICAD/common/eda_units.cpp" -o eda_units.o
$CXX -c "$KICAD/thirdparty/fmt/src/format.cc" -o fmt_format.o

LIBS="libeval_compiler.o numeric_evaluator.o string_utils.o eda_units.o fmt_format.o \
      $(wx-config --libs base) -lharfbuzz"

$CXX "$HERE/oracle_libeval.cpp" $LIBS -o oracle_libeval
$CXX "$HERE/oracle_numeric.cpp" $LIBS -o oracle_numeric

echo "built $OUT/oracle_libeval and $OUT/oracle_numeric"
