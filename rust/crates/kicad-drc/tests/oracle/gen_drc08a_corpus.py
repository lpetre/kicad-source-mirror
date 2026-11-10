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
"""DRC-08a goldens: the copper clearance error types of the reference DRC reports.

Two goldens, one line per report entry of the types `clearance`, `hole_clearance`,
`shorting_items` and `tracks_crossing`, in report order:

    @ <path>
    <index> TAB <type code> TAB <uuid of item 1> TAB <uuid of item 2> TAB <hash>

(type codes C clearance, H hole_clearance, S shorting_items, X tracks_crossing; UUIDs cut to
their first 8 characters to keep the golden small) where <hash> is the FNV-1a 64 of the entry text of `gen_drc_corpus.py` (`hash_line`: severity,
type, description, per item uuid, description and position in IU). Boards without such entries
get `@ <path> TAB none`, failed reference runs `@ <path> TAB load_error`. The item UUIDs let the
Rust replay attribute each entry to the phase that produced it (DRC-08a item tests, DRC-08b
graphics / zones, the physical clearance provider).

* `tests/golden/drc08a/corpus.txt`: the cached default reports of `gen_drc_corpus.py`
  (`kicad-cli pcb drc --format json --severity-all --units mm`, rust/target/drc-corpus/; run
  that script first).
* `tests/golden/drc08a/all_track_errors.txt`: the same command with `--all-track-errors` on
  the boards listed in ALL_TRACK_ERRORS (cached under rust/target/drc08a-ate/).
"""

import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gen_drc_corpus as corpus  # noqa: E402

TYPES = {"clearance": "C", "hole_clearance": "H", "shorting_items": "S", "tracks_crossing": "X"}
GOLDEN_DIR = os.path.join(HERE, "..", "golden", "drc08a")
ATE_CACHE = os.path.join(corpus.KICAD, "rust", "target", "drc08a-ate")

# Small boards with track / via / pad clearance violations (drc.md 10.2 DRC-08a row and the
# corpus boards with entries of these types).
ALL_TRACK_ERRORS = [
    "qa/data/pcbnew/issue1358.kicad_pcb",
    "qa/data/pcbnew/issue2528.kicad_pcb",
    "qa/data/pcbnew/issue5854.kicad_pcb",
    "qa/data/pcbnew/issue7241.kicad_pcb",
    "qa/data/pcbnew/issue7267.kicad_pcb",
    "qa/data/pcbnew/issue8003.kicad_pcb",
    "qa/data/pcbnew/issue9081.kicad_pcb",
    "qa/data/pcbnew/reverse_via.kicad_pcb",
    "qa/data/pcbnew/issue24355/issue24355.kicad_pcb",
    "qa/data/pcbnew/issue17429.kicad_pcb",
    "qa/data/pcbnew/issue5093.kicad_pcb",
    "demos/pic_programmer/pic_programmer.kicad_pcb",
]


def lines_for(rel, report):
    if report is None:
        return [f"@ {rel}\tload_error"]

    out = []

    for i, v in enumerate(report.get("violations", [])):
        if v["type"] not in TYPES:
            continue

        uuids = [item["uuid"][:8] for item in v.get("items", [])] + ["", ""]
        h = corpus.fnv1a64(corpus.FNV_OFFSET, corpus.hash_line(v).encode())
        out.append(f"{i}\t{TYPES[v['type']]}\t{uuids[0]}\t{uuids[1]}\t{h:016x}")

    return [f"@ {rel}"] + out if out else [f"@ {rel}\tnone"]


def run_ate(cli, rel, env):
    """`run_board` of gen_drc_corpus.py with `--all-track-errors` and its own cache."""
    saved_cache, saved_run = corpus.CACHE, corpus.subprocess.run

    def run(cmd, **kw):
        return saved_run(cmd[:6] + ["--all-track-errors"] + cmd[6:], **kw)

    corpus.CACHE, corpus.subprocess.run = ATE_CACHE, run

    try:
        return corpus.run_board(cli, rel, env)
    finally:
        corpus.CACHE, corpus.subprocess.run = saved_cache, saved_run


def write(path, lines):
    os.makedirs(os.path.dirname(path), exist_ok=True)

    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")

    print(f"{len(lines)} lines -> {path}")


def main():
    cli = "/home/user/kicad-oracle/bin/kicad-cli"
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="drc08a-home-"))
    lines = []

    for rel in sorted(corpus.corpus()):
        cached = os.path.join(corpus.CACHE, rel + ".json")

        if not os.path.exists(cached):
            lines.append(f"@ {rel}\tload_error")
            continue

        with open(cached) as f:
            lines += lines_for(rel, json.load(f))

    write(os.path.join(GOLDEN_DIR, "corpus.txt"), lines)

    lines = []

    for rel in ALL_TRACK_ERRORS:
        print(f"--all-track-errors {rel}", flush=True)
        lines += lines_for(rel, run_ate(cli, rel, env))

    write(os.path.join(GOLDEN_DIR, "all_track_errors.txt"), lines)


if __name__ == "__main__":
    main()
