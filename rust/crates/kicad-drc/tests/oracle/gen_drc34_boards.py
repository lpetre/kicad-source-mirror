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

"""Synthetic net chain boards of package DRC-34 and their reference report order.

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc34_boards.py [KICAD_CLI]

From qa/data/pcbnew/drc_net_chain_tuning_profiles the board and project are rewritten into
tests/data/drc34/chains_<n>.kicad_pcb / .kicad_pro: the net_chains block is replaced by n
chains of two nets each (names below, non-ASCII and bracketed ones included); the first net
of every chain is assigned to ADDITIONAL_NETCLASS (tuning profile ADDITIONAL_PROFILE) in the
project, the second keeps Default (no profile), so every chain is reported. The order of the
reports is the iteration order of the C++ std::unordered_map keyed by chain name, which the
port reproduces (n = 14 and 17 rehash past 13 buckets).

The reference kicad-cli (default /home/user/kicad-oracle/bin/kicad-cli) runs
`pcb drc --format json --severity-all` on a copy; the descriptions of the
net_chain_tuning_profiles entries, in report order, go to tests/golden/drc34/chains.txt as
`<case>\\t<description>` lines.

The via-stitch case tests/data/drc34/stitch_overlap.kicad_pcb is qa/data/pcbnew/issue25303
with its one via_stitch generator (net GND, "A") copied four times: B (GND, shifted 5 mm so
it overlaps A), C (net Net-(U1Power-V+), overlapping A), D (GND, 100 mm away) and E (GND,
overlapping A and B). Its via_stitch_overlap entries go to tests/golden/drc34/stitch.txt as
`<description>\\t<item uuids>\\t<item positions>` lines in report order.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
SRC = os.path.join(KICAD, "qa", "data", "pcbnew", "drc_net_chain_tuning_profiles",
                   "test_drc_net_chain_tuning_profiles")
DATA = os.path.join(HERE, "..", "data", "drc34")
GOLDEN = os.path.join(HERE, "..", "golden", "drc34", "chains.txt")
STITCH_SRC = os.path.join(KICAD, "qa", "data", "pcbnew", "issue25303")
STITCH_GOLDEN = os.path.join(HERE, "..", "golden", "drc34", "stitch.txt")

NAMES = ["CHAIN_1", "CHAIN_2", "bus[3]", "DDR_DQ0", "Ünïcode", "a", "b", "zz-top", "CLK",
         "USB_D", "PCIE_TX0", "PCIE_RX0", "net chain", "X9", "MIPI_CSI", "LVDS+", "eth0",
         "eth1", "SPI_MOSI", "I2C_SDA"]


def nets(board_text):
    seen = []
    for m in re.finditer(r'\(net "([^"]*)"\)', board_text):
        n = m.group(1)
        if n and n not in seen:
            seen.append(n)
    return seen


def make_case(n, board_text, project):
    all_nets = nets(board_text)
    assert len(all_nets) >= 2 * n, (len(all_nets), n)
    chains = []
    assignments = {}

    for i in range(n):
        a, b = all_nets[2 * i], all_nets[2 * i + 1]
        chains.append((NAMES[i], [a, b]))
        assignments[a] = ["ADDITIONAL_NETCLASS"]

    block = "\t(net_chains\n"
    for name, members in chains:
        block += "\t\t(net_chain\n\t\t\t(name \"%s\")\n\t\t\t(members\n" % name
        for m in members:
            block += "\t\t\t\t(net \"%s\")\n" % m
        block += "\t\t\t)\n\t\t)\n"
    block += "\t)\n"

    text = re.sub(r"\t\(net_chains\n.*?\n\t\)\n", lambda _: block, board_text, flags=re.S)
    assert text != board_text

    proj = json.loads(json.dumps(project))
    proj["net_settings"]["netclass_assignments"] = assignments
    proj["net_settings"]["net_chain_netclasses"] = {}
    return text, proj


def shifted_generator(block, uuid, dx, dy, net):
    """A copy of the generated block with a new uuid, the outline moved and another net."""
    out = re.sub(r'\(uuid "[^"]*"\)', '(uuid "%s")' % uuid, block, count=1)
    out = out.replace('(net_name "GND")', '(net_name "%s")' % net)

    def move(m):
        return "(xy %.6f %.6f)" % (float(m.group(1)) + dx, float(m.group(2)) + dy)

    head, sep, rest = out.partition("(outline")
    body, sep2, tail = rest.partition("(pitch")
    body = re.sub(r"\(xy ([-0-9.]+) ([-0-9.]+)\)", move, body)
    # the template via gets a fresh uuid as well
    tail = re.sub(r'\(uuid "[^"]*"\)', '(uuid "%s")' % (uuid[:-1] + "f"), tail, count=1)
    return head + sep + body + sep2 + tail


def run_cli(cli, pcb, pro):
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copy(pcb, tmp)
        shutil.copy(pro, tmp)
        env = dict(os.environ)
        env["KICAD_ORACLE_HOME"] = os.path.join(tmp, "home")
        report = os.path.join(tmp, "out.json")
        subprocess.run([cli, "pcb", "drc", "--format", "json", "--severity-all", "-o", report,
                        os.path.join(tmp, os.path.basename(pcb))],
                       env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return json.load(open(report, encoding="utf-8"))


def stitch_case(cli):
    text = open(STITCH_SRC + ".kicad_pcb", encoding="utf-8").read()
    start = text.index("\t(generated\n")
    end = text.index("\n\t)\n", start) + len("\n\t)\n")
    block = text[start:end]
    copies = [
        ("00000000-0000-0000-0034-0000000000b0", 5.0, 0.0, "GND"),
        ("00000000-0000-0000-0034-0000000000c0", 2.0, 2.0, "Net-(U1Power-V+)"),
        ("00000000-0000-0000-0034-0000000000d0", 100.0, 0.0, "GND"),
        ("00000000-0000-0000-0034-0000000000e0", 2.5, 3.0, "GND"),
    ]
    extra = "".join(shifted_generator(block, u, dx, dy, net) for u, dx, dy, net in copies)
    text = text[:end] + extra + text[end:]
    pcb = os.path.join(DATA, "stitch_overlap.kicad_pcb")
    pro = os.path.join(DATA, "stitch_overlap.kicad_pro")
    open(pcb, "w", encoding="utf-8").write(text)
    shutil.copy(STITCH_SRC + ".kicad_pro", pro)
    doc = run_cli(cli, pcb, pro)
    out = []

    for v in doc.get("violations", []):
        if v["type"] == "via_stitch_overlap":
            out.append("%s\t%s\t%s" % (
                v["description"], ",".join(i["uuid"] for i in v["items"]),
                ";".join("%s,%s" % (i["pos"]["x"], i["pos"]["y"]) for i in v["items"])))

    open(STITCH_GOLDEN, "w", encoding="utf-8").write("\n".join(out) + "\n")
    print("wrote %d lines to %s" % (len(out), STITCH_GOLDEN))


def main():
    cli = sys.argv[1] if len(sys.argv) > 1 else "/home/user/kicad-oracle/bin/kicad-cli"
    board_text = open(SRC + ".kicad_pcb", encoding="utf-8").read()
    project = json.load(open(SRC + ".kicad_pro", encoding="utf-8"))
    os.makedirs(DATA, exist_ok=True)
    os.makedirs(os.path.dirname(GOLDEN), exist_ok=True)
    out = []

    for n in (3, 14, 17):
        case = "chains_%d" % n
        text, proj = make_case(n, board_text, project)
        pcb = os.path.join(DATA, case + ".kicad_pcb")
        pro = os.path.join(DATA, case + ".kicad_pro")
        open(pcb, "w", encoding="utf-8").write(text)
        open(pro, "w", encoding="utf-8").write(json.dumps(proj, indent=2) + "\n")

        with tempfile.TemporaryDirectory() as tmp:
            shutil.copy(pcb, tmp)
            shutil.copy(pro, tmp)
            env = dict(os.environ)
            env["KICAD_ORACLE_HOME"] = os.path.join(tmp, "home")
            report = os.path.join(tmp, "out.json")
            subprocess.run([cli, "pcb", "drc", "--format", "json", "--severity-all", "-o",
                            report, os.path.join(tmp, case + ".kicad_pcb")],
                           env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            doc = json.load(open(report, encoding="utf-8"))

        for v in doc.get("violations", []):
            if v["type"] == "net_chain_tuning_profiles":
                out.append("%s\t%s" % (case, v["description"]))

    open(GOLDEN, "w", encoding="utf-8").write("\n".join(out) + "\n")
    print("wrote %d lines to %s" % (len(out), GOLDEN))

    stitch_case(cli)


if __name__ == "__main__":
    main()
