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

"""DRC-23 golden: the reference entries of the misc and footprint checks providers' types.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc23_golden.py

Reads the raw reference reports cached by gen_drc_corpus.py (rust/target/drc-corpus/<path>.json,
`kicad-cli pcb drc --format json --severity-all --units mm`) and writes
tests/golden/drc23/corpus.txt: per board of the shared corpus golden that has a cached report,
one line `<path> TAB cached`, then one line per entry of the types below in report order, in the
format of drc_corpus.txt (`<path> TAB <index> TAB <severity> TAB <type> TAB <description>
{ TAB <uuid> TAB <item description> TAB <x> TAB <y> }`). The board item of `SetItems( m_board )`
(description "PCB") carries a fresh random UUID per reference run: it is written as uuid `board`.
`shorting_items` (also reported by the copper clearance provider) is written only for boards
with net-tie pad groups (the file mentions `net_tie_pad_groups`); the replay keeps the entries
whose items all belong to one net-tie footprint.
"""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../.."))
CACHE = os.path.join(KICAD, "rust/target/drc-corpus")
GOLDEN = os.path.join(HERE, "../golden/drc_corpus.txt")
OUT = os.path.join(HERE, "../golden/drc23/corpus.txt")

TYPES = {
    "invalid_outline", "item_on_disabled_layer", "unresolved_variable", "assertion_failure",
    "generic_warning", "generic_error", "missing_tuning_profile", "footprint_type_mismatch",
    "footprint_scaled_with_pads", "through_hole_pad_without_hole", "padstack", "padstack_invalid",
    "shorting_items", "footprint",
}


def severity(v):
    return v["severity"] + ("/excluded" if v.get("excluded") else "")


def main():
    boards = []

    with open(GOLDEN) as f:
        for line in f:
            rel = line.split("\t", 1)[0]

            if not boards or boards[-1] != rel:
                boards.append(rel)

    out = []

    for rel in boards:
        cached = os.path.join(CACHE, rel + ".json")

        if not os.path.exists(cached):
            continue

        with open(cached) as f:
            report = json.load(f)

        out.append(f"{rel}\tcached")

        with open(os.path.join(KICAD, rel), errors="replace") as f:
            has_net_ties = "net_tie_pad_groups" in f.read()

        for i, v in enumerate(report.get("violations", [])):
            if v["type"] not in TYPES:
                continue

            if v["type"] == "shorting_items" and not has_net_ties:
                continue

            fields = [rel, str(i), severity(v), v["type"], v["description"]]

            for item in v.get("items", []):
                uuid = "board" if item["description"] == "PCB" else item["uuid"]
                fields += [uuid, item["description"], repr(item["pos"]["x"]),
                           repr(item["pos"]["y"])]

            out.append("\t".join(f.replace("\t", " ").replace("\n", " ") for f in fields))

    os.makedirs(os.path.dirname(OUT), exist_ok=True)

    with open(OUT, "w") as f:
        f.write("\n".join(out) + "\n")

    print(f"{len(boards)} boards, {sum(1 for l in out if l.endswith(chr(9) + 'cached'))} cached, "
          f"{sum(1 for l in out if not l.endswith(chr(9) + 'cached'))} entries")


if __name__ == "__main__":
    main()
