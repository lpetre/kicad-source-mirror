#!/usr/bin/env bash
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

# The performance-phase gate (docs/PERF.md): proof that an optimisation changed no output.
#
#   perf-gate.sh full                     every entry below
#   perf-gate.sh changed <crate>...       the entries of the named crates and of every
#                                         workspace crate that depends on them (transitively,
#                                         from `cargo metadata`)
#   perf-gate.sh list                     print the entries
#
# Options (after the mode):
#   --only <entry-id-substring>           run only matching entries (debugging the gate)
#   --bless                               (re)write the expected snapshots of the report
#                                         entries from this run; only after the run was
#                                         checked to be identical to the reference
#
# Every entry runs `cargo test` in the dev profile (workspace: opt-level 1). An entry is one of
#   * a crate's whole default test suite (all non-ignored unit / integration / doc tests: the
#     committed goldens and oracle replays), or
#   * one ignored corpus replay, run with `--ignored --exact`.
# Some replays only write a scoreboard (they do not assert identity); for those the gate
# extracts the deterministic status lines (timings stripped) and compares them with the
# committed snapshot in scripts/perf-gate/expected/<entry>.txt, so any board whose status
# changes fails the gate. Entries that read reference caches under rust/target check that the
# cache is there first and FAIL (never skip) when it is missing.
#
# Output: one line per entry (PASS / FAIL, seconds), a summary line, exit code 1 on any
# failure. Full log: target/perf/gate-<timestamp>.log; per-entry output:
# target/perf/gate-<timestamp>/<entry>.out.

set -u -o pipefail

RUST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KICAD_DIR="$(cd "$RUST_DIR/.." && pwd)"
TARGET="$RUST_DIR/target"
EXPECTED_DIR="$RUST_DIR/scripts/perf-gate/expected"

# ---- entries ----------------------------------------------------------------------------------
#
# id | crate | caches needed | env | cargo args (after `cargo test -p <crate>`) | test args
#    | snapshot (empty, or `file:<path under rust/>` or `out`, then `@<filter>`)
#
# Caches: zone-corpus (target/zone-corpus/*/ref, tools/zone-corpus/run.sh --refs-only),
# io-corpus (target/io-corpus/boards, tools/io-corpus/run.sh), oracle-cli (the reference
# kicad-cli, /home/user/kicad-oracle/bin/kicad-cli or KICAD_ORACLE_CLI), zint-bin
# (target/zint-oracle/zint_oracle, the C driver of crates/kicad-zint/tests/oracle).
ENTRIES=(
# foundation crates: unit tests, C++ test ports, oracle replays of committed goldens
"sexpr|kicad-sexpr|||--no-fail-fast||"
"sexpr.big_corpus|kicad-sexpr|||--test corpus|--ignored --exact big_corpus_matches_cpp|"
"geom|kicad-geom|||--no-fail-fast||"
"core|kicad-core|||--no-fail-fast||"
"libeval|kicad-libeval|||--no-fail-fast||"
"font|kicad-font|||--no-fail-fast||"
"zint|kicad-zint|||--no-fail-fast||"
"zint.driver|kicad-zint|zint-bin|ZINT_ORACLE_BIN=$TARGET/zint-oracle/zint_oracle ZINT_ORACLE_FROM=0 ZINT_ORACLE_TO=20000|--test zint_oracle|--ignored --exact zint_cases_vs_driver|"
"plot|kicad-plot|||--no-fail-fast||"
# board model, reader / writer: goldens, dump goldens (feature dump), io corpus replays
"pcb|kicad-pcb|||--no-fail-fast --features dump||"
"pcb.corpus_large|kicad-pcb|||--lib|--ignored --exact io::parser::tests::corpus_large|"
"pcb.vcu118|kicad-pcb|||--test pcbio03_tracks_shapes|--ignored --exact collinear_track_arc_loads_as_track_vcu118|"
"pcb.io_corpus|kicad-pcb|io-corpus,oracle-cli|IO_CORPUS_JOBS=2|--test io_corpus|--ignored --exact corpus_scoreboard|file:target/io-corpus/scoreboard.md@iocorpus"
"pcb.text_blocks|kicad-pcb|io-corpus||--test pcbio05_text|--ignored --exact corpus_text_blocks|"
"pcb.dimension_blocks|kicad-pcb|io-corpus||--test pcbio10_dimensions|--ignored --exact corpus_dimension_blocks|"
"pcb.d3_legacy|kicad-pcb|io-corpus,oracle-cli||--features dump --test int2b_review|--ignored --exact d3_legacy_scoreboard|file:target/int2b/d3.md@plain"
# connectivity
"connectivity|kicad-connectivity|||--no-fail-fast||"
"connectivity.cn04_corpus|kicad-connectivity|||--test cn04_data_golden|--ignored --exact cn04_corpus_build --nocapture|out@cn04"
# zone fill
"zone|kicad-zone|zone-corpus|ZONE_CORPUS_STRICT=1|--no-fail-fast||file:target/zone-corpus/report.txt@plain"
"zone.teardrop_corpus|kicad-zone|zone-corpus||--test td01_teardrops|--ignored --exact teardrop_corpus|file:target/zone-corpus/td01_report.txt@plain"
# DRC: providers, rules, engine / evalrules / report oracles
"drc|kicad-drc|||--no-fail-fast||"
# fabrication outputs: plot, Excellon, Gerber drill, drill map corpora
"fab|kicad-fab|||--no-fail-fast||"
# session: upgrade, refill + save, DRC job, Gerber / SVG jobs, board API
"session|kicad-session|zone-corpus||--no-fail-fast||"
"session.upgrade_corpus|kicad-session|io-corpus,oracle-cli||--test upgrade_corpus|--ignored --exact upgrade_corpus_scoreboard|file:target/io-corpus/session_scoreboard.md@plain"
"session.zf06_refill_save|kicad-session|zone-corpus||--test zf06_refill_save|--ignored --exact zf06_refill_save_corpus|file:target/zone-corpus/save_report.txt@plain"
# CLI end to end against the committed reference hashes
"cli|kicad-cli|||--no-fail-fast||"
"cli.e2e_all|kicad-cli|||--test e2e_cli|--ignored --exact e2e_identical_pairs_all|"
# wasm crate (native build of its tests), bench harness, qa helpers
"wasm|kicad-wasm|||--no-fail-fast||"
"bench|kicad-bench|||--no-fail-fast||"
"qa|kicad-qa|||--no-fail-fast||"
)

# Ignored tests deliberately NOT in the gate (stale or not output tests), for the record:
#   kicad-libeval test_libeval_compiler (21, "needs PCB binding": run in kicad-drc instead),
#   kicad-libeval / numeric_evaluator dump_generated_cases (exporters for the C++ driver),
#   kicad-drc report_oracle::reference_cli_reports (stub; superseded by session drc12_corpus),
#   kicad-drc drc32 creepage_stadium_caps_block_outer_side (un-ignored copy lives in drc33),
#   kicad-drc drc33 drc33_debug_one / drc33_oracle_full_dump (diagnostics),
#   kicad-drc drc33 creepage_performance_issue21482 (a timing assertion, not an output test),
#   kicad-pcb containers create_footprint_has_the_four_mandatory_fields (M10 pending),
#   kicad-pcb pcbio13 oracle_large_board (needs PCBIO13_BOARD / PCBIO13_REF by hand),
#   kicad-pcb board_design_settings_json pcb_dump_ds_lines (helper for a python oracle),
#   kicad-session component_classes (2, need DRC-22 functions).

# ---- snapshot filters (stdin: report text, stdout: deterministic status lines) -------------------

filter_plain() { cat; }

filter_iocorpus() {
    # The oracle build stamp changes with every oracle rebuild, not with the port.
    grep -v '^Oracle build stamp'
}

filter_cn04() {
    # Per-board lines and the summary of cn04_corpus_build, timings removed.
    grep -E '(unconnected, [0-9]+ dangling$|: PANIC |^cn04 corpus: )' |
        sed -E 's/: [0-9.]+ ms, /: /; s/ built in [0-9.]+ ms,/ built,/'
}

# ---- helpers ------------------------------------------------------------------------------------

die() {
    echo "perf-gate: $*" >&2
    exit 2
}

# check_cache <token>: prints why the cache is unusable, returns 1 then.
check_cache() {
    case "$1" in
    zone-corpus)
        local n
        n=$(find "$TARGET/zone-corpus" -mindepth 2 -maxdepth 2 -type d -name ref 2>/dev/null | wc -l)
        [ "$n" -gt 0 ] || { echo "missing cache target/zone-corpus/*/ref (rust/tools/zone-corpus/run.sh --refs-only)"; return 1; }
        ;;
    io-corpus)
        local n
        n=$(find "$TARGET/io-corpus/boards" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l)
        [ "$n" -gt 0 ] || { echo "missing cache target/io-corpus/boards (rust/tools/io-corpus/run.sh)"; return 1; }
        ;;
    oracle-cli)
        local cli="${KICAD_ORACLE_CLI:-/home/user/kicad-oracle/bin/kicad-cli}"
        [ -x "$cli" ] || { echo "missing reference kicad-cli $cli (docs/ORACLE.md)"; return 1; }
        ;;
    zint-bin)
        [ -x "$TARGET/zint-oracle/zint_oracle" ] || { echo "missing target/zint-oracle/zint_oracle (crates/kicad-zint/tests/oracle)"; return 1; }
        ;;
    "") ;;
    *) echo "unknown cache token $1"; return 1 ;;
    esac
    return 0
}

# Workspace crates that depend (transitively, any dependency kind) on one of the given crates,
# including the given ones.
dependents_closure() {
    (cd "$RUST_DIR" && cargo metadata --format-version 1 --no-deps 2>/dev/null) |
        python3 -I -c '
import json, sys
meta = json.load(sys.stdin)
names = {p["name"] for p in meta["packages"]}
rdeps = {n: set() for n in names}
for p in meta["packages"]:
    for d in p["dependencies"]:
        if d["name"] in names:
            rdeps[d["name"]].add(p["name"])
todo = list(sys.argv[1:])
for t in todo:
    if t not in names:
        sys.exit("unknown workspace crate: " + t)
seen = set(todo)
while todo:
    for r in rdeps[todo.pop()]:
        if r not in seen:
            seen.add(r)
            todo.append(r)
print("\n".join(sorted(seen)))
' "$@"
}

# ---- arguments ----------------------------------------------------------------------------------

MODE="${1:-}"
[ -n "$MODE" ] || die "usage: perf-gate.sh full | changed <crate>... | list  [--only <id>] [--bless]"
shift

CRATES=()
ONLY=""
BLESS=0

while [ $# -gt 0 ]; do
    case "$1" in
    --only) ONLY="$2"; shift 2 ;;
    --bless) BLESS=1; shift ;;
    -*) die "unknown option $1" ;;
    *) CRATES+=("$1"); shift ;;
    esac
done

case "$MODE" in
full | list) [ ${#CRATES[@]} -eq 0 ] || die "$MODE takes no crate names" ;;
changed)
    [ ${#CRATES[@]} -gt 0 ] || die "changed needs at least one crate name"
    # Accept `zone` for `kicad-zone`.
    for i in "${!CRATES[@]}"; do
        case "${CRATES[$i]}" in kicad-*) ;; *) CRATES[$i]="kicad-${CRATES[$i]}" ;; esac
    done
    mapfile -t SELECTED < <(dependents_closure "${CRATES[@]}") || die "cargo metadata failed"
    [ ${#SELECTED[@]} -gt 0 ] || die "no crates selected"
    ;;
*) die "unknown mode $MODE" ;;
esac

selected() {
    local crate="$1" id="$2"
    if [ -n "$ONLY" ] && [[ "$id" != *"$ONLY"* ]]; then
        return 1
    fi
    [ "$MODE" != changed ] && return 0
    local c
    for c in "${SELECTED[@]}"; do
        [ "$c" = "$crate" ] && return 0
    done
    return 1
}

if [ "$MODE" = list ]; then
    for e in "${ENTRIES[@]}"; do
        IFS='|' read -r id crate needs envs cargo_args test_args snap <<<"$e"
        printf '%-28s %-20s needs=%-22s cargo test -p %s %s -- %s %s\n' \
            "$id" "$crate" "${needs:--}" "$crate" "$cargo_args" "$test_args" "${snap:+[snapshot $snap]}"
    done
    exit 0
fi

# ---- run ----------------------------------------------------------------------------------------

STAMP="$(date +%Y%m%d-%H%M%S)"
mkdir -p "$TARGET/perf"
LOG="$TARGET/perf/gate-$STAMP.log"
OUT_DIR="$TARGET/perf/gate-$STAMP"
mkdir -p "$OUT_DIR"

COMMIT="$(git -C "$RUST_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)"
DIRTY="$(git -C "$RUST_DIR" status --porcelain -- . 2>/dev/null | wc -l)"

log() { echo "$*" | tee -a "$LOG"; }

log "perf-gate $MODE ${CRATES[*]:-} at $COMMIT ($DIRTY changed paths under rust/), $(date -Is)"
[ "$MODE" = changed ] && log "crates: ${SELECTED[*]}"

PASS=0
FAIL=0
FAILED=()
T0=$SECONDS

for e in "${ENTRIES[@]}"; do
    IFS='|' read -r id crate needs envs cargo_args test_args snap <<<"$e"
    selected "$crate" "$id" || continue

    out="$OUT_DIR/$id.out"
    t=$SECONDS
    start_epoch=$(date +%s)
    status=PASS
    why=""

    # Caches first: missing ones fail loudly.
    IFS=',' read -r -a tokens <<<"$needs"
    for tok in "${tokens[@]}"; do
        if ! msg=$(check_cache "$tok"); then
            status=FAIL
            why="$msg"
        fi
    done

    if [ "$status" = PASS ]; then
        # shellcheck disable=SC2086 # word splitting of the arg strings is intended
        (
            cd "$RUST_DIR" || exit 99
            exec env $envs cargo test -p "$crate" $cargo_args -- $test_args
        ) >"$out" 2>&1
        rc=$?

        if [ $rc -ne 0 ]; then
            status=FAIL
            why="cargo test exit $rc"
        elif ! grep -qE '^test result: ok\. [0-9]+ passed' "$out"; then
            status=FAIL
            why="no test ran"
        elif [ -n "$test_args" ] && grep -qE '^test result: ok\. 0 passed' "$out" &&
            ! grep -qE '^test result: ok\. [1-9][0-9]* passed' "$out"; then
            status=FAIL
            why="the named test was not found"
        fi

        # A replay that refreshed the io-corpus oracle cache ran the reference: say so.
        if grep -qE '^\[oracle [0-9]+/[0-9]+\]' "$out"; then
            why="${why:+$why; }oracle cache was refreshed during the run"
        fi

        # Scoreboard entries: compare the status lines with the committed snapshot.
        if [ "$status" = PASS ] && [ -n "$snap" ]; then
            src="${snap%@*}"
            filter="filter_${snap##*@}"
            got="$OUT_DIR/$id.snapshot"

            case "$src" in
            out) $filter <"$out" >"$got" ;;
            file:*)
                f="$RUST_DIR/${src#file:}"
                if [ ! -f "$f" ] || [ "$(stat -c %Y "$f")" -lt "$start_epoch" ]; then
                    status=FAIL
                    why="report ${src#file:} not written by this run"
                else
                    $filter <"$f" >"$got"
                fi
                ;;
            esac

            if [ "$status" = PASS ]; then
                exp="$EXPECTED_DIR/$id.txt"
                if [ $BLESS -eq 1 ]; then
                    mkdir -p "$EXPECTED_DIR"
                    cp "$got" "$exp"
                    why="${why:+$why; }snapshot blessed"
                elif [ ! -f "$exp" ]; then
                    status=FAIL
                    why="no expected snapshot scripts/perf-gate/expected/$id.txt (run with --bless once the run is verified)"
                elif ! diff -u "$exp" "$got" >"$OUT_DIR/$id.diff"; then
                    status=FAIL
                    why="snapshot differs ($(grep -c '^[-+][^-+]' "$OUT_DIR/$id.diff") lines, see $OUT_DIR/$id.diff)"
                else
                    rm -f "$OUT_DIR/$id.diff"
                fi
            fi
        fi
    fi

    dt=$((SECONDS - t))

    if [ "$status" = PASS ]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        FAILED+=("$id")
    fi

    line=$(printf '%-4s %-28s %6ss  %s' "$status" "$id" "$dt" "$why")
    log "$line"

    if [ "$status" = FAIL ] && [ -f "$out" ]; then
        {
            echo "---- $id: failing tests / last lines"
            grep -E '^test .* FAILED$|^---- .* stdout ----$|panicked at' "$out" | head -40
            tail -n 15 "$out"
            echo "----"
        } >>"$LOG"
    fi
done

TOTAL=$((SECONDS - T0))
log "SUMMARY $MODE: $PASS passed, $FAIL failed, ${TOTAL}s ($((TOTAL / 60)) min) at $COMMIT${FAILED:+; failed: ${FAILED[*]}}"
log "log: $LOG"

[ $FAIL -eq 0 ]
