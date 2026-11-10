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

"""Regenerate tests/golden/drc34/evalrules_report.txt (the EvalRules REPORTER texts).

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc34_report_golden.py [--raw REL] [ORACLE]

ORACLE defaults to /home/user/kicad-oracle/bin/drc34-report-oracle
(tests/oracle/build_drc34_report_oracle.sh). Boards: every .kicad_pcb under <kicad>/qa/data/pcbnew
and <kicad>/demos smaller than 300 kB, and the review boards tests/data/review/rev_*.kicad_pcb.
Each board and its .kicad_pro / .kicad_dru are copied to a temporary directory and the oracle
runs on the copy, one board at a time; its output is streamed and reduced to one line per
board and category:

    <path relative to <kicad>>|<category>|n=<lines>|hash=<FNV-1a 64>

where the hash covers "<key>|<field>|<messages>\\n" of the category's `r|` lines in output
order. Boards the oracle cannot load get `<path>|load_error`. With `--raw REL` the oracle's
lines of that one board are printed instead (for diagnosing a mismatch).
"""

import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
GOLDEN = os.path.join(HERE, "..", "golden", "drc34", "evalrules_report.txt")
MAX_SIZE = 300 * 1024
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3


def fnv1a64(h, data):
    for b in data:
        h ^= b
        h = (h * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return h


def corpus():
    for root in ("qa/data/pcbnew", "demos"):
        for dirpath, dirs, files in os.walk(os.path.join(KICAD, root)):
            dirs.sort()
            for f in sorted(files):
                if f.endswith(".kicad_pcb"):
                    p = os.path.join(dirpath, f)
                    if os.path.getsize(p) < MAX_SIZE:
                        yield os.path.relpath(p, KICAD)

    review = os.path.join(HERE, "..", "data", "review")
    for f in sorted(os.listdir(review)):
        if f.startswith("rev_") and f.endswith(".kicad_pcb"):
            yield os.path.relpath(os.path.join(review, f), KICAD)


def run(tool, rel, raw):
    src = os.path.join(KICAD, rel)
    base = os.path.splitext(src)[0]
    cats = {}
    loaded = True

    with tempfile.TemporaryDirectory() as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
            if os.path.exists(base + ext):
                shutil.copy(base + ext, tmp)

        env = dict(os.environ)
        env["HOME"] = os.path.join(tmp, "home")
        proc = subprocess.Popen([tool, os.path.join(tmp, os.path.basename(src))],
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env,
                                cwd=tmp)

        for line in proc.stdout:
            if raw:
                sys.stdout.write(line.decode("utf-8", "replace"))
                continue

            if line.startswith(b"error|load"):
                loaded = False
            if not line.startswith(b"r|"):
                continue

            parts = line.rstrip(b"\n").split(b"|", 2)

            if len(parts) != 3:
                sys.stderr.write("%s: unexpected line %r\n" % (rel, line[:200]))
                continue

            _, cat, rest = parts
            n, h = cats.get(cat, (0, FNV_OFFSET))
            cats[cat] = (n + 1, fnv1a64(h, rest + b"\n"))

        proc.wait()

    if raw:
        return []

    if not loaded or proc.returncode != 0:
        return ["%s|load_error" % rel]

    return ["%s|%s|n=%d|hash=%016x" % (rel, c.decode(), n, h) for c, (n, h) in sorted(cats.items())]


def main():
    args = sys.argv[1:]
    raw = None

    if args and args[0] == "--raw":
        raw = args[1]
        args = args[2:]

    tool = args[0] if args else "/home/user/kicad-oracle/bin/drc34-report-oracle"

    if raw:
        run(tool, raw, True)
        return

    out = []

    for rel in corpus():
        out.extend(run(tool, rel, False))

    os.makedirs(os.path.dirname(GOLDEN), exist_ok=True)
    open(GOLDEN, "w", encoding="utf-8").write("\n".join(out) + "\n")
    print("wrote %d lines to %s" % (len(out), GOLDEN))


if __name__ == "__main__":
    main()
