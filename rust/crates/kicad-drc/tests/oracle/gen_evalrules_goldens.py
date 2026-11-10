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

"""Regenerate tests/golden/evalrules_corpus.txt of kicad-drc with the C++ reference dump tool.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_evalrules_goldens.py [--out GOLDEN] [PCB_DUMP] [BOARD...]

The review boards (tests/data/review, see gen_review_boards.py) have their own golden:

    python3 -I .../gen_evalrules_goldens.py --out .../tests/golden/evalrules_review.txt \\
        /home/user/kicad-oracle/bin/pcb-dump rust/crates/kicad-drc/tests/data/review/rev_*.kicad_pcb

PCB_DUMP defaults to /home/user/kicad-oracle/bin/pcb-dump (rust/tools/pcb-dump/build.sh). For
every .kicad_pcb under <kicad>/qa/data/pcbnew and <kicad>/demos smaller than 5 MB (or only the
given boards, paths relative to <kicad>), the board and its sibling .kicad_pro / .pro /
.kicad_dru are copied to a temporary directory and
`pcb-dump board --hash --sections evalrules` (QA load path, see
rust/tools/pcb-dump/dump_evalrules.cpp) runs on the copy, sequentially. The output is streamed
and reduced to one golden line per board and key category (track, arc, via, pad, zone, fpzone,
drawing, disallow, pair_tt, pair_pp, pair_tp, pair_zt, pair_zp, pair_zz):

    <path relative to <kicad>>|<category>|n=<keys>|lines=<lines>|hash=<FNV-1a 64>

where the hash covers "<field>=<hashed value>\\n" of the category's lines in output order (item
keys are left out: they are ordinals for items without a stored UUID). A board line
`<path>|board|errors=<error lines>` precedes them; boards that fail to load get `|load_error`.
"""

import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
GOLDEN = os.path.join(HERE, "..", "golden", "evalrules_corpus.txt")
MAX_SIZE = 5 * 1024 * 1024
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3


def fnv1a64(h: int, data: bytes) -> int:
    for b in data:
        h ^= b
        h = (h * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return h


def corpus():
    for root in ("qa/data/pcbnew", "demos"):
        for dirpath, _, files in os.walk(os.path.join(KICAD, root)):
            for f in sorted(files):
                if f.endswith(".kicad_pcb"):
                    p = os.path.join(dirpath, f)
                    if os.path.getsize(p) < MAX_SIZE:
                        yield os.path.relpath(p, KICAD)


def dump_board(tool, rel, env):
    src = os.path.join(KICAD, rel)
    base = os.path.splitext(src)[0]
    cats = {}
    errors = 0
    loaded = False

    with tempfile.TemporaryDirectory(prefix="evalrules-") as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".pro", ".kicad_dru"):
            if os.path.exists(base + ext):
                shutil.copy(base + ext, os.path.join(tmp, os.path.basename(base) + ext))
        board = os.path.join(tmp, os.path.basename(src))
        proc = subprocess.Popen([tool, "board", "--hash", "--sections", "evalrules", board],
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)
        last_key = {}
        for raw in proc.stdout:
            line = raw.rstrip(b"\n")
            if line.startswith(b"file|"):
                loaded = True
                continue
            if line.startswith(b"error|"):
                if line.startswith(b"error|load|"):
                    loaded = False
                errors += 1
                continue
            if not line.startswith(b"evalrules|"):
                continue
            _, key, rest = line.split(b"|", 2)
            cat = key.split(b":", 1)[0].decode()
            c = cats.setdefault(cat, [0, 0, FNV_OFFSET])
            if last_key.get(cat) != key:
                c[0] += 1
                last_key[cat] = key
            c[1] += 1
            c[2] = fnv1a64(c[2], rest + b"\n")
        try:
            proc.wait(timeout=600)
        except subprocess.TimeoutExpired:
            proc.kill()
            return None

    if not loaded:
        return None

    out = [f"{rel}|board|errors={errors}"]
    for cat in sorted(cats):
        n, lines, h = cats[cat]
        out.append(f"{rel}|{cat}|n={n}|lines={lines}|hash={h:016x}")
    return out


def main():
    args = sys.argv[1:]
    golden = GOLDEN

    if "--out" in args:
        i = args.index("--out")
        golden = args[i + 1]
        del args[i:i + 2]

    tool = args[0] if args else "/home/user/kicad-oracle/bin/pcb-dump"
    boards = args[1:] or sorted(corpus())
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="evalrules-home-"))
    out = []

    for rel in boards:
        lines = dump_board(tool, rel, env)
        out.extend(lines if lines is not None else [f"{rel}|load_error"])

    with open(golden, "w") as f:
        f.write("\n".join(out) + "\n")
    print(f"{len(boards)} boards -> {golden}")


if __name__ == "__main__":
    main()
