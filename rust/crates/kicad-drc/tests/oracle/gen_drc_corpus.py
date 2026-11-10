#!/usr/bin/env python3
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

"""The shared DRC corpus of the provider packages (DRC-06 and later): reference violations.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc_corpus.py [--cli KICAD_CLI] [BOARD...]

For every .kicad_pcb under <kicad>/qa/data/pcbnew and <kicad>/demos smaller than 5 MB except the large
demo boards of PORTING.md section 7 (or only
the given boards, paths relative to <kicad>) the board and its sibling .kicad_pro / .kicad_dru /
.kicad_prl are copied to a scratch directory and the reference runs on the copy, sequentially:

    kicad-cli pcb drc --format json --severity-all --units mm -o out.json board.kicad_pcb

The raw report (date removed) is cached as rust/target/drc-corpus/<path>.json (re-used when the
board is older than the cache; delete the directory to force a rerun). The committed golden
tests/golden/drc_corpus.txt holds, per board, one line per entry of `violations` in report
order (tab separated, so that descriptions may contain anything but tabs and newlines):

    <path relative to <kicad>> TAB <index> TAB <severity> TAB <type> TAB <description>
        { TAB <uuid> TAB <item description> TAB <x> TAB <y> }

with x / y exactly as printed by the reference (mm), for the error types of the ported
providers (PORTED_TYPES below: a provider package adds its types and reruns the script, which
re-uses the cached reports) with at most MAX_FULL entries on the board. Every other type gets
one summary line per board

    <path> TAB # TAB <type> TAB <count> TAB <FNV-1a 64 of the entry texts in report order>

where an entry text is severity, type, description and per item uuid, description, x, y with
the position in IU (round( mm * 1e6 )), tab separated, ending in a newline. The golden stays
small; the severity of an entry excluded in the project gets the suffix `/excluded` (such
entries are sorted after the others). The replay prints the entries of a mismatching summary from the cached report. Boards whose run fails get one line `<path> TAB load_error`,
boards without violations `<path> TAB none`. The Rust replay (tests/drc_harness) filters both
sides to the error types of the providers under test.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
GOLDEN = os.path.join(HERE, "..", "golden", "drc_corpus.txt")
CACHE = os.path.join(KICAD, "rust", "target", "drc-corpus")
MAX_SIZE = 5 * 1024 * 1024
# Error types whose entries are stored in full (providers that have been ported).
PORTED_TYPES = {
    # DRC-06
    "annular_width", "drill_out_of_range", "microvia_drill_out_of_range", "track_width",
    "via_diameter",
}
# More entries of one type on one board are stored as a summary line only.
MAX_FULL = 20
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3


def fnv1a64(h, data):
    for b in data:
        h ^= b
        h = (h * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return h
# Large demo boards that must not run next to other jobs (PORTING.md section 7).
EXCLUDED = ("openair-max", "tiny_tapeout", "cm5_minima", "jetson", "vme-wren")


def corpus():
    for root in ("qa/data/pcbnew", "demos"):
        for dirpath, _, files in os.walk(os.path.join(KICAD, root)):
            for f in sorted(files):
                if f.endswith(".kicad_pcb"):
                    p = os.path.join(dirpath, f)
                    if os.path.getsize(p) < MAX_SIZE and not any(x in p for x in EXCLUDED):
                        yield os.path.relpath(p, KICAD)


def run_board(cli, rel, env):
    src = os.path.join(KICAD, rel)
    cached = os.path.join(CACHE, rel + ".json")

    if os.path.exists(cached) and os.path.getmtime(cached) >= os.path.getmtime(src):
        with open(cached) as f:
            return json.load(f)

    base = os.path.splitext(src)[0]

    with tempfile.TemporaryDirectory(prefix="drc-corpus-") as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru", ".kicad_prl"):
            if os.path.exists(base + ext):
                shutil.copy(base + ext, os.path.join(tmp, os.path.basename(base) + ext))

        board = os.path.join(tmp, os.path.basename(src))
        out = os.path.join(tmp, "out.json")

        try:
            subprocess.run(["nice", "-n", "10", cli, "pcb", "drc", "--format", "json",
                            "--severity-all", "--units", "mm", "-o", out, board],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env,
                           timeout=900, cwd=tmp)
        except subprocess.TimeoutExpired:
            return None

        if not os.path.exists(out):
            return None

        with open(out) as f:
            report = json.load(f)

    report.pop("date", None)
    os.makedirs(os.path.dirname(cached), exist_ok=True)

    with open(cached, "w") as f:
        json.dump(report, f, indent=1)

    return report


def severity(v):
    """The severity field: `/excluded` appended for entries excluded in the project."""
    return v["severity"] + ("/excluded" if v.get("excluded") else "")


def hash_line(v):
    """The text an entry contributes to a summary hash (positions in IU, see the docstring)."""
    fields = [severity(v), v["type"], v["description"]]

    for item in v.get("items", []):
        fields += [item["uuid"], item["description"], str(round(item["pos"]["x"] * 1e6)),
                   str(round(item["pos"]["y"] * 1e6))]

    return "\t".join(f.replace("\t", " ").replace("\n", " ") for f in fields) + "\n"


def golden_lines(rel, report):
    if report is None:
        return [f"{rel}\tload_error"]

    violations = report.get("violations", [])
    counts = {}

    for v in violations:
        counts[v["type"]] = counts.get(v["type"], 0) + 1

    out = []
    summary = {}

    for i, v in enumerate(violations):
        t = v["type"]

        if t in PORTED_TYPES and counts[t] <= MAX_FULL:
            fields = [rel, str(i), severity(v), t, v["description"]]

            for item in v.get("items", []):
                fields += [item["uuid"], item["description"], repr(item["pos"]["x"]),
                           repr(item["pos"]["y"])]

            out.append("\t".join(f.replace("\t", " ").replace("\n", " ") for f in fields))
        else:
            n, h = summary.get(t, (0, FNV_OFFSET))
            summary[t] = (n + 1, fnv1a64(h, hash_line(v).encode()))

    for t in sorted(summary):
        n, h = summary[t]
        out.append(f"{rel}\t#\t{t}\t{n}\t{h:016x}")

    return out or [f"{rel}\tnone"]


def main():
    args = sys.argv[1:]
    cli = "/home/user/kicad-oracle/bin/kicad-cli"

    if args[:1] == ["--cli"]:
        cli = args[1]
        args = args[2:]

    boards = args or sorted(corpus())
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="drc-corpus-home-"))
    lines = {}

    if args and os.path.exists(GOLDEN):
        with open(GOLDEN) as f:
            for line in f:
                rel = line.split("\t", 1)[0]
                lines.setdefault(rel, []).append(line.rstrip("\n"))

    for n, rel in enumerate(boards):
        print(f"[{n + 1}/{len(boards)}] {rel}", flush=True)
        lines[rel] = golden_lines(rel, run_board(cli, rel, env))

    with open(GOLDEN, "w") as f:
        for rel in sorted(lines):
            f.write("\n".join(lines[rel]) + "\n")

    print(f"{len(boards)} boards -> {GOLDEN}")


if __name__ == "__main__":
    main()
