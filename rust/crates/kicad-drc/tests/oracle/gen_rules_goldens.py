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

"""Regenerate the rule parser goldens of kicad-drc with the C++ reference dump tool.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_rules_goldens.py [PCB_DUMP]

PCB_DUMP defaults to /home/user/kicad-oracle/bin/pcb-dump (built by rust/tools/pcb-dump/build.sh,
command `rules`, format in rust/docs/PCB_DUMP_FORMAT.md section 7). Inputs are copied to a
temporary directory first. Writes into tests/golden/:

* rules_corpus.txt: every .kicad_dru under <kicad>/qa and <kicad>/demos, compact form (per file
  and mode: status, rule count, message count and the FNV-1a hash of the mode's dump lines);
* rules_errors.txt: tests/data/rules/*.kicad_dru (deliberately broken files): the dump lines
  without the rule fields (status, errors, messages, counts) plus the compact form;
* rules_cc.txt: the same for tests/data/rules/cc/*.kicad_dru with --component-classes.

`warn|` lines (wx assertions counted during a parse) are dropped. Files listed in
SKIP_CXX make the C++ parser loop forever and are not run (see PORT_STATUS.md).
"""

import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.dirname(os.path.dirname(HERE))
KICAD = os.path.abspath(os.path.join(CRATE, "..", "..", ".."))
GOLDEN = os.path.join(CRATE, "tests", "golden")
DATA = os.path.join(CRATE, "tests", "data", "rules")
SKIP_CXX = {"return_path_unknown_eof.kicad_dru"}


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def run_dump(pcb_dump, paths, extra=()):
    """Run pcb-dump rules on copies of paths; returns one list of lines per input."""
    with tempfile.TemporaryDirectory() as tmp:
        copies = []
        for i, p in enumerate(paths):
            d = os.path.join(tmp, str(i))
            os.mkdir(d)
            c = os.path.join(d, os.path.basename(p))
            shutil.copy(p, c)
            copies.append(c)
        out = subprocess.run([pcb_dump, "rules", *extra, *copies], cwd=tmp, check=True,
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                             timeout=600).stdout.decode("utf-8")
    per_file = []
    for line in out.split("\n"):
        if line.startswith("file|"):
            per_file.append([])
        elif line and not line.startswith("warn|"):
            per_file[-1].append(line)
    assert len(per_file) == len(paths), (len(per_file), len(paths))
    return per_file


def compact(lines):
    result = []
    for mode in ("report", "throw"):
        prefix = "rules|%s|" % mode
        ml = [l for l in lines if l.startswith(prefix)]

        def field(name):
            for l in ml:
                if l.startswith(prefix + name + "="):
                    return l[len(prefix + name + "="):]
            return "?"

        msgs = sum(1 for l in ml if l.startswith(prefix + "msg."))
        joined = "".join(l + "\n" for l in ml).encode("utf-8")
        result.append("%s|status=%s|count=%s|msgs=%d|#%016x"
                      % (mode, field("status"), field("count"), msgs, fnv1a64(joined)))
    return result


def summary(lines):
    return [l for l in lines if "|rule." not in l] + compact(lines)


def main():
    pcb_dump = sys.argv[1] if len(sys.argv) > 1 else "/home/user/kicad-oracle/bin/pcb-dump"

    corpus = []
    for top in ("qa", "demos"):
        for root, _dirs, files in os.walk(os.path.join(KICAD, top)):
            for f in files:
                if f.endswith(".kicad_dru"):
                    corpus.append(os.path.relpath(os.path.join(root, f), KICAD))
    corpus.sort()

    with open(os.path.join(GOLDEN, "rules_corpus.txt"), "w") as out:
        out.write("# pcb-dump rules over every .kicad_dru of qa/ and demos/ (compact form), "
                  "see tests/oracle/gen_rules_goldens.py\n")
        dumps = run_dump(pcb_dump, [os.path.join(KICAD, p) for p in corpus])
        for rel, lines in zip(corpus, dumps):
            out.write("file|%s|\n" % rel)
            for l in compact(lines):
                out.write(l + "\n")

    for name, sub, extra in (("rules_errors.txt", "", ()),
                             ("rules_cc.txt", "cc", ("--component-classes",))):
        d = os.path.join(DATA, sub)
        files = sorted(f for f in os.listdir(d) if f.endswith(".kicad_dru") and f not in SKIP_CXX)
        dumps = run_dump(pcb_dump, [os.path.join(d, f) for f in files], extra)
        with open(os.path.join(GOLDEN, name), "w") as out:
            for f, lines in zip(files, dumps):
                out.write("file|%s|\n" % f)
                for l in summary(lines):
                    out.write(l + "\n")

    print("corpus files: %d" % len(corpus))


if __name__ == "__main__":
    main()
