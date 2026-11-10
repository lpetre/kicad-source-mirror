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

"""Regenerate tests/golden/engine_corpus.txt of kicad-drc with the C++ reference dump tool.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_engine_goldens.py [PCB_DUMP] [FULL_DIR]

PCB_DUMP defaults to /home/user/kicad-oracle/bin/pcb-dump (rust/tools/pcb-dump/build.sh). For
every .kicad_pcb under <kicad>/qa/data/pcbnew and <kicad>/demos smaller than 5 MB, the board
and its sibling .kicad_pro / .pro / .kicad_dru are copied to a temporary directory and
`pcb-dump board --sections engine` (QA load path, format: rust/docs/PCB_DUMP_FORMAT.md
section 9) runs on the copy. One golden line per board:

    <path relative to <kicad>>|lines=<n>|hash=<FNV-1a 64 of the engine lines>|drc_error=<0/1>|
    keepout=<n>|tuning=<n>|user=<n>|user_cond=<n>

where the counts are rules by implicit source (keepout, tuning profile) and user rules (with a
condition). Boards that fail to load get `|load_error`. With FULL_DIR the full dumps are also
written there (one file per board) for diffing; keep them out of the source tree.
"""

import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
GOLDEN = os.path.join(HERE, "..", "golden", "engine_corpus.txt")
MAX_SIZE = 5 * 1024 * 1024


def fnv1a64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def corpus():
    for root in ("qa/data/pcbnew", "demos"):
        for dirpath, _, files in os.walk(os.path.join(KICAD, root)):
            for f in sorted(files):
                if f.endswith(".kicad_pcb"):
                    p = os.path.join(dirpath, f)
                    if os.path.getsize(p) < MAX_SIZE:
                        yield os.path.relpath(p, KICAD)


def main():
    tool = sys.argv[1] if len(sys.argv) > 1 else "/home/user/kicad-oracle/bin/pcb-dump"
    full_dir = sys.argv[2] if len(sys.argv) > 2 else None
    out = []
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="engine-home-"))

    for rel in sorted(corpus()):
        src = os.path.join(KICAD, rel)
        base = os.path.splitext(src)[0]
        with tempfile.TemporaryDirectory(prefix="engine-") as tmp:
            for ext in (".kicad_pcb", ".kicad_pro", ".pro", ".kicad_dru"):
                if os.path.exists(base + ext):
                    shutil.copy(base + ext, os.path.join(tmp, os.path.basename(base) + ext))
            board = os.path.join(tmp, os.path.basename(src))
            try:
                res = subprocess.run([tool, "board", "--sections", "engine", board],
                                     capture_output=True, timeout=300, env=env)
                text = res.stdout.decode("utf-8", "surrogateescape")
            except subprocess.TimeoutExpired:
                text = ""
        lines = text.splitlines()
        engine = [l for l in lines if l.startswith("engine|")]
        drc_error = any(l.startswith("error|load|drc|") for l in lines)
        if not engine:
            out.append(f"{rel}|load_error")
            continue
        data = "".join(l + "\n" for l in engine).encode("utf-8", "surrogateescape")
        keepout = sum(1 for l in engine if l.endswith("|source=KEEPOUT"))
        tuning = sum(1 for l in engine if l.endswith("|source=TUNING_PROFILE"))
        user = sum(1 for l in engine if l.endswith("|source=NONE"))
        user_cond = 0
        rules = {}
        for l in engine:
            parts = l.split("|", 2)
            if parts[1].startswith("rule."):
                rules.setdefault(parts[1], []).append(parts[2])
        for fields in rules.values():
            if "source=NONE" in fields and "condition=none" not in fields:
                user_cond += 1
        out.append(f"{rel}|lines={len(engine)}|hash={fnv1a64(data):016x}|drc_error={int(drc_error)}"
                   f"|keepout={keepout}|tuning={tuning}|user={user}|user_cond={user_cond}")
        if full_dir:
            os.makedirs(full_dir, exist_ok=True)
            with open(os.path.join(full_dir, rel.replace("/", "__") + ".txt"), "w",
                      encoding="utf-8", errors="surrogateescape") as f:
                f.write("\n".join(engine) + "\n")

    with open(GOLDEN, "w") as f:
        f.write("\n".join(out) + "\n")
    print(f"{len(out)} boards -> {GOLDEN}")


if __name__ == "__main__":
    main()
