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
#
# Package FIX-EDIT-1: condenses the stdout of editmut_oracle (stdin) and its saved boards
# (OUTDIR) into the golden: per edit, board and dump section one line
#   <edit>|<board>|<section>=<fnv1a64 of the sorted value lines>:<line count>
# and per edit and board
#   <edit>|<board>|save=<fnv1a64 of the saved file>:<byte count>
# tests/fix_edit1.rs computes the same hashes. With KEEP=<dir> the raw dump of every
# (edit, board) is kept there for debugging (never commit it).
import os
import sys


def fnv(data: bytes) -> str:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h


def flush(out, head, groups, keep):
    if head is None:
        return
    edit, board = head
    for section in sorted(groups):
        lines = sorted(groups[section])
        data = "\n".join(lines).encode()
        out.write("%s|%s|%s=%s:%d\n" % (edit, board, section, fnv(data), len(lines)))
    if keep:
        with open(os.path.join(keep, "%s__%s.dump" % (edit, board)), "w") as f:
            for section in sorted(groups):
                f.write("\n".join(sorted(groups[section])) + "\n")


def main():
    outdir = sys.argv[1]
    keep = os.environ.get("KEEP")
    out = sys.stdout
    head, groups, saves = None, {}, []
    for raw in sys.stdin:
        line = raw.rstrip("\n")
        if line.startswith("@@|"):
            flush(out, head, groups, keep)
            _, edit, board, asserts = line.split("|")
            head, groups = (edit, board), {}
            saves.append((edit, board))
            out.write("%s|%s|%s\n" % (edit, board, asserts))
            continue
        if not line or line.startswith("file|") or line.startswith("#"):
            continue
        section = line.split("|", 1)[0]
        groups.setdefault(section, []).append(line)
    flush(out, head, groups, keep)
    for edit, board in saves:
        data = open(os.path.join(outdir, "%s__%s" % (edit, board)), "rb").read()
        out.write("%s|%s|save=%s:%d\n" % (edit, board, fnv(data), len(data)))


main()
