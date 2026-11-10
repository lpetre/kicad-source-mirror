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

"""DRC-29 golden: the copper_sliver and connection_width entries of the cached reference
reports of gen_drc_corpus.py (rust/target/drc-corpus/<path>.json), in report order:

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc29_golden.py

writes tests/golden/drc29/corpus.txt, one line per entry (tab separated):

    <path> TAB <severity> TAB <type> TAB <description> { TAB <uuid> TAB <item description> TAB <x> TAB <y> }

and `<path> TAB none` for a board without such entries; boards whose reference run failed are
left out. Re-run after gen_drc_corpus.py refreshed the cache.
"""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
CACHE = os.path.join(KICAD, "rust", "target", "drc-corpus")
OUT = os.path.join(HERE, "..", "golden", "drc29", "corpus.txt")
TYPES = ("copper_sliver", "connection_width")

lines = []

for root, _, files in os.walk(CACHE):
    for f in files:
        if not f.endswith(".kicad_pcb.json"):
            continue

        path = os.path.join(root, f)
        rel = os.path.relpath(path, CACHE)[: -len(".json")]

        try:
            doc = json.load(open(path))
        except Exception:
            continue

        entries = []

        for v in doc.get("violations", []):
            if v["type"] not in TYPES:
                continue

            severity = v["severity"] + ("/excluded" if v.get("excluded") else "")
            fields = [rel, severity, v["type"], v["description"]]

            for it in v["items"]:
                fields += [it["uuid"], it["description"], repr(it["pos"]["x"]), repr(it["pos"]["y"])]

            entries.append("\t".join(x.replace("\t", " ").replace("\n", " ") for x in fields))

        lines += entries if entries else [rel + "\tnone"]

lines.sort(key=lambda l: l.split("\t")[0])

with open(OUT, "w") as out:
    out.write("\n".join(lines) + "\n")

print(f"{len(lines)} lines -> {OUT}")
