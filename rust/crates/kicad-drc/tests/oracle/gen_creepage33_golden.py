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
"""Regenerates tests/golden/creepage33_{board,engine}.txt with creepage33-oracle
(build_creepage33_oracle.sh) on copies of the creepage QA boards.

The golden keeps the solve lines of the dump (`S|...` / `E|...`) with the item UUIDs cut to
their first 8 hex digits and, for `S` lines, the FNV-1a 64 hash of the path lines (`C|...`)
appended. Usage: gen_creepage33_golden.py [--keep DIR]
"""
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "../../../../.."))
GOLDEN = os.path.join(HERE, "../golden")
ORACLE = "/home/user/kicad-oracle/bin/creepage33-oracle"
BOARDS = ["creepage/creepage", "creepage/creepage_malformed_edge", "creepage_slots/creepage_slots",
          "issue20480/issue20480", "issue21482/issue21482", "issue23389/issue23389",
          "issue23578/issue23578", "issue23658/issue23658", "issue24078/issue24078",
          "issue24286/issue24286", "issue24523/issue24523", "issue24543/issue24543",
          "issue24544/issue24544", "issue24597/issue24597", "issue24853/issue24853",
          "issue24875/issue24875", "issue24875_capstraight/issue24875_capstraight",
          "issue24875_fullround/issue24875_fullround", "issue24885/issue24885",
          "issue25439/issue25439"]


def fnv1a(text):
    h = 0xCBF29CE484222325
    for b in text.encode():
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h


def short(u):
    return u if u == "-" else u[:8]


def compact(dump, tag):
    out, pending, path = [], None, []

    def flush():
        if pending is not None:
            out.append(pending + ("|" + fnv1a("\n".join(path)) if tag == "S" else ""))

    for line in dump.splitlines():
        f = line.split("|")
        if f[0] == "C":
            path.append(line)
            continue
        flush()
        pending, path = None, []
        if f[0] == tag:
            # the item fields: S|a|b|layer|target|dist|n|item1|item2|...,
            # E|a|b|layer|dist|itemA|itemB|...
            items = (7, 8) if tag == "S" else (5, 6)
            for k in items:
                f[k] = short(f[k])
            pending = "|".join(f)
        else:
            out.append(line)
    flush()
    return "\n".join(out) + "\n"


def main():
    work = tempfile.mkdtemp(prefix="creepage33-")
    files = []
    for b in BOARDS:
        d, name = b.split("/")
        dst = os.path.join(work, d)
        os.makedirs(dst, exist_ok=True)
        for ext in ("kicad_pcb", "kicad_pro", "kicad_dru"):
            src = os.path.join(KICAD, "qa/data/pcbnew", d, name + "." + ext)
            if os.path.exists(src):
                shutil.copy(src, dst)
        files.append(os.path.join(dst, name + ".kicad_pcb"))

    env = dict(os.environ, KICAD_CONFIG_HOME=os.path.join(work, "config"))
    board = subprocess.run([ORACLE, "board"] + files, capture_output=True, text=True,
                           env=env).stdout
    engine = subprocess.run([ORACLE, "engine"] + [f for f in files if "21482" not in f],
                            capture_output=True, text=True, env=env).stdout
    open(os.path.join(GOLDEN, "creepage33_board.txt"), "w").write(compact(board, "S"))
    open(os.path.join(GOLDEN, "creepage33_engine.txt"), "w").write(compact(engine, "E"))

    if "--keep" in sys.argv:
        keep = sys.argv[sys.argv.index("--keep") + 1]
        open(os.path.join(keep, "board.txt"), "w").write(board)
    shutil.rmtree(work)


if __name__ == "__main__":
    main()
