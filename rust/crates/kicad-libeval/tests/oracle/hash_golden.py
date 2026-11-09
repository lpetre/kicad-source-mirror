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
"""Reduce the output of a reference driver to one FNV-1a (32 bit) hash per case.

Usage: hash_golden.py < oracle_output.txt > tests/golden/<name>_fuzz_hashes.txt

Works for the output of oracle_libeval and of oracle_numeric.  The hash covers the lines
between "EXPR ..." and "END" (each with its newline), which is the text the run_case()
helpers of the Rust tests reproduce.
"""

import sys


def fnv1a32(data):
    h = 0x811C9DC5
    for b in data:
        h ^= b
        h = (h * 0x01000193) & 0xFFFFFFFF
    return h


def main():
    out = sys.stdout
    out.write("# FNV-1a (32 bit) of the C++ reference output for each case of generated_cases() in\n")
    out.write("# the test that reads this file; written by tests/oracle/hash_golden.py.\n")
    block = None
    for raw in sys.stdin.buffer:
        if raw.startswith(b"EXPR\t"):
            block = b""
        elif raw == b"END\n":
            out.write("%08x\n" % fnv1a32(block))
            block = None
        elif block is not None:
            block += raw


if __name__ == "__main__":
    main()
