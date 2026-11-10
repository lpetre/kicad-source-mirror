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

"""Package M26 golden: generated tables refreshed by the reference `kicad-cli pcb upgrade --force`.

Usage: m26_generated_tables.py KICAD_CLI WORKDIR GOLDEN

Builds the inputs of tests/data/m26/cases.txt (the Rust test tests/m26_generated_tables.rs builds
the same ones), upgrades copies with the reference kicad-cli, upgrades the result once more (O2, the
fixed-point check; a board that is no fixed point also gets the summary of O2) and writes per board and per generated block (drill charts, stackup tables, the
setup's drill symbol profile) an FNV-1a hash of the block with new cell UUIDs normalised, plus the
column widths, row heights and cell texts in clear for diagnosis. The full outputs stay in WORKDIR.
"""

import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data", "m26")
QA = os.path.join(HERE, "..", "..", "..", "..", "..", "qa", "data", "pcbnew")


def fnv1a64(text):
    h = 0xCBF29CE484222325
    for b in text.encode("utf-8"):
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h


def cases():
    out = []
    for line in open(os.path.join(DATA, "cases.txt")):
        if line.startswith("#") or not line.strip():
            continue
        out.append([f.strip() for f in line.split("|")])
    return out


def normalise(text, original):
    """New UUIDs (not in the input) -> 00000000-0000-4000-8000-<ordinal>, in order."""
    n = 0
    lines = []
    for line in text.split("\n"):
        t = line.lstrip("\t")
        if t.startswith('(uuid "') and t not in original:
            n += 1
            line = line[: len(line) - len(t)] + '(uuid "00000000-0000-4000-8000-%012d")' % n
        lines.append(line)
    return "\n".join(lines)


def blocks(text):
    """Top-level drill_chart / stackup_table blocks and the setup's drill_symbol_profile."""
    out = []
    cur = None
    ind = ""
    for line in text.split("\n"):
        if cur is None:
            if (line.startswith("\t(drill_chart") or line.startswith("\t(stackup_table")
                    or line.startswith("\t\t(drill_symbol_profile")):
                cur = [line]
                ind = line[: len(line) - len(line.lstrip("\t"))]
        else:
            cur.append(line)
            if line == ind + ")":
                out.append("\n".join(cur) + "\n")
                cur = None
    return out


def stage1_input(index, base, profile):
    text = open(os.path.join(QA, base)).read().rstrip()
    if profile != "-":
        prof = open(os.path.join(DATA, profile)).read().rstrip("\n")
        idx = text.index("(setup") + len("(setup")
        text = text[:idx] + "\n" + prof + text[idx:]
    tables = open(os.path.join(DATA, "tables.txt")).read()
    for k in range(10):
        tables = tables.replace("@UUID%d@" % k, "%08x-0000-4000-8000-%012d" % (0x26000000 + index, k))
    return text[:-1] + tables + ")\n"


def stage2_input(base, mode, gen1_blocks):
    tables = "".join(b for b in gen1_blocks if not b.startswith("\t\t("))
    if mode == "legacy":
        # Legacy chart: no row keys (rows migrate by position, the totals row by role)
        tables = re.sub(r"\t\t\(row_keys\n(\t\t\t\(key [^\n]*\n)*\t\t\)\n", "", tables)
    elif mode == "styled":
        # Every 1.27 mm cell bold with 2 mm text and other margins (donors of new rows)
        tables = tables.replace("(size 1.27 1.27)", "(size 2 2)\n\t\t\t\t\t\t(bold yes)")
        tables = tables.replace("(margins 1.0025 1.0025 1.0025 1.0025)", "(margins 0.7 0.2 0.3 0.4)")
    text = open(os.path.join(QA, base)).read().rstrip()
    return text[:-1] + tables + ")\n"


def upgrade(cli, work, path):
    env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(work, "home"))
    subprocess.run([cli, "pcb", "upgrade", "--force", path], env=env, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=600)
    return open(path).read()


def summary(name, out, original):
    norm = normalise(out, original)
    lines = []
    bl = blocks(norm)
    for i, b in enumerate(bl):
        kind = b.strip().split("\n")[0].strip().lstrip("(")
        lines.append("block|%s|%d|%s|fnv=%s" % (name, i, kind, fnv1a64(b)))
        for key in ("column_widths", "row_heights"):
            m = re.search(r"\n\t\t\(%s ([^)]*)\)" % key, b)
            if m:
                lines.append("%s|%s|%d|%s" % (key, name, i, " ".join(m.group(1).split())))
        texts = re.findall(r'\(table_cell "((?:[^"\\]|\\.)*)"', b)
        if texts:
            lines.append("texts|%s|%d|%s" % (name, i, "\t".join(texts)))
    return lines, bl, norm


def main():
    cli, work, golden = sys.argv[1:4]
    os.makedirs(work, exist_ok=True)
    out_lines = ["# Package M26: generated tables after the reference kicad-cli pcb upgrade --force",
                 "# (tests/oracle/m26_generated_tables.py; cases: tests/data/m26/cases.txt)"]
    gen1 = {}
    for index, (name, base, profile, mode) in enumerate(cases()):
        if mode == "tables":
            text = stage1_input(index, base, profile)
        elif mode == "-":
            text = open(os.path.join(QA, base)).read()
        else:
            text = stage2_input(base, mode, gen1["m26_backdrill"])
        path = os.path.join(work, name + ".kicad_pcb")
        open(os.path.join(work, name + ".input.kicad_pcb"), "w").write(text)
        open(path, "w").write(text)
        o1 = upgrade(cli, work, path)
        open(path, "w").write(o1)
        o2 = upgrade(cli, work, path)
        lines, bl, norm = summary(name, o1, text)
        gen1[name] = bl
        open(os.path.join(work, name + ".o1.kicad_pcb"), "w").write(norm)
        open(os.path.join(work, name + ".o2.kicad_pcb"), "w").write(normalise(o2, text))
        fixed = "yes" if normalise(o2, o1) == o1 else "no"
        out_lines.append("case|%s|blocks=%d|fixed_point=%s" % (name, len(bl), fixed))
        out_lines.extend(lines)
        if fixed == "no":
            # The second refresh moves something: its result is compared as well
            out_lines.extend(summary(name + "@o2", o2, text)[0])
    open(golden, "w").write("\n".join(out_lines) + "\n")


if __name__ == "__main__":
    main()
