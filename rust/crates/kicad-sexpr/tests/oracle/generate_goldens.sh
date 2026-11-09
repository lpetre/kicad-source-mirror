#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of the KiCad Rust port.  Copyright The KiCad Developers, see AUTHORS.txt for contributors.
#
# Regenerate the golden files in ../golden with the oracle harness (see oracle.cpp, build.sh).
#
# usage: generate_goldens.sh [oracle binary]   (default: <kicad>/rust/target/kicad-sexpr-oracle/oracle)
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD=$(cd "$HERE/../../../../.." && pwd)
ORACLE=${1:-$KICAD/rust/target/kicad-sexpr-oracle/oracle}
GOLDEN=$HERE/../golden

"$ORACLE" script "$GOLDEN/lexer_script.txt" > "$GOLDEN/lexer_script.out"
"$ORACLE" numbers "$GOLDEN/numbers.txt" > "$GOLDEN/numbers.out"
"$ORACLE" printf "$GOLDEN/printf.txt" > "$GOLDEN/printf.out"
"$ORACLE" quotes "$GOLDEN/quotes.txt" > "$GOLDEN/quotes.out"
"$ORACLE" sexpr "$GOLDEN/sexpr.txt" > "$GOLDEN/sexpr.out"
"$ORACLE" sexprapi > "$GOLDEN/sexpr_api.out"
"$ORACLE" prettifylines "$GOLDEN/prettify.txt" > "$GOLDEN/prettify.out"
# wxWidgets asserts on the empty buffer of the first FormatStreamData case; that goes to stderr.
"$ORACLE" format > "$GOLDEN/format.out" 2> /dev/null

: > "$GOLDEN/keywords.out"

for set in pcb_keys_t pcbplotparams_t strokeparams_t embedded_files_t drcrule_t lib_table_t \
           drawingsheet_t nl_t tfield_t dsn; do
    echo "# $set" >> "$GOLDEN/keywords.out"
    "$ORACLE" keywords "$set" >> "$GOLDEN/keywords.out"
done

# The corpus: real files of the KiCad tree, chosen deterministically.
(
    cd "$KICAD"
    echo "# Files of the KiCad source tree used by tests/corpus.rs, relative to its root."
    echo "# Written by tests/oracle/generate_goldens.sh."
    # all boards of the pcbnew test data and of the demos
    find qa/data/pcbnew demos -name '*.kicad_pcb' | LC_ALL=C sort
    # every 8th footprint, every 3rd symbol library
    find qa/data -name '*.kicad_mod' | LC_ALL=C sort | awk 'NR % 8 == 1'
    find qa/data -name '*.kicad_sym' | LC_ALL=C sort | awk 'NR % 3 == 1'
    # all custom rule files and drawing sheets
    find qa/data -name '*.kicad_dru' | LC_ALL=C sort
    find qa/data demos -name '*.kicad_wks' | LC_ALL=C sort
    # library tables
    for table in sym-lib-table fp-lib-table nested-symbols nested-disabled nested-hidden cycle \
                 sym-hand-edited; do
        echo "qa/data/libraries/$table"
    done
) > "$GOLDEN/corpus_files.txt"

"$ORACLE" corpus "$GOLDEN/corpus_files.txt" "$KICAD" > "$GOLDEN/corpus.out"

ls -la "$GOLDEN"
