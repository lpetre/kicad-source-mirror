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

"""DRC-31 golden: the length / skew / via count / net chain entries of the reference
kicad-cli DRC report for every case:

    python3 -I rust/crates/kicad-drc/tests/oracle/gen_drc31_golden.py

A case is a board of qa/data/pcbnew copied to rust/target/drc31/<case>/ with its .kicad_pro,
and a rule file: the board's own (`base`), the board's own with tests/data/drc31/<v>.dru
appended (`append:<v>`), or tests/data/drc31/<v>.dru alone (`replace:<v>`). Cases with net
chains get a `(net_chains ...)` block inserted before the board's last `)`: the C++ QA tests
assign chains and terminal pads in code; the CLI reads them from the file. The block is
written to tests/data/drc31/<case>.chains (the Rust test inserts the same text).

Writes tests/data/drc31/cases.tsv (case, board, rule mode, chains file) and
tests/golden/drc31/reference.txt, one line per entry in report order (tab separated):

    <case> TAB <severity> TAB <type> TAB <description> { TAB <uuid> TAB <item description> TAB <x> TAB <y> }

and `<case> TAB none` for a case without such entries.
"""

import json
import os
import re
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
KICAD = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
DATA = os.path.join(HERE, "..", "data", "drc31")
SCRATCH = os.path.join(KICAD, "rust", "target", "drc31")
OUT = os.path.join(HERE, "..", "golden", "drc31", "reference.txt")
CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
TYPES = ("length_out_of_range", "skew_out_of_range", "too_many_vias",
         "net_chain_stub_length", "net_chain_return_path")

QA = "qa/data/pcbnew/"


# ---- a minimal s-expression reader (enough for footprints, pads and nets) ----------------

def tokenize(text):
    return re.findall(r'"(?:[^"\\]|\\.)*"|\(|\)|[^\s()"]+', text)


def parse(tokens):
    stack = [[]]

    for t in tokens:
        if t == "(":
            stack.append([])
        elif t == ")":
            done = stack.pop()
            stack[-1].append(done)
        else:
            stack[-1].append(t[1:-1] if t.startswith('"') else t)

    return stack[0][0]


def children(node, name):
    return [c for c in node if isinstance(c, list) and c and c[0] == name]


def footprints(board):
    """[(x, [(pad uuid, net name)])] in file order."""
    out = []

    for fp in children(board, "footprint"):
        at = (children(fp, "at") or [["at", "0", "0"]])[0]
        pads = []

        for pad in children(fp, "pad"):
            net = children(pad, "net")
            uuid = children(pad, "uuid")
            netname = net[0][-1] if net else ""
            pads.append((uuid[0][1] if uuid else "", netname))

        out.append((float(at[1]), pads))

    return out


def last_pad_of_net(fps, net):
    found = None

    for _, pads in fps:
        for uuid, n in pads:
            if n == net:
                found = uuid

    return found


def fp_by_x(fps, which):
    with_pads = [f for f in fps if f[1]]
    first = None
    last = None

    for f in with_pads:
        if first is None or f[0] < first[0]:
            first = f

        if last is None or f[0] > last[0]:
            last = f

    return (first if which == "min" else last)[1][0][0]


def chains_block(rel, spec):
    text = open(os.path.join(KICAD, rel)).read()
    board = parse(tokenize(text))
    fps = footprints(board)
    nets = sorted({c[-1] for c in children(board, "net")} | {n for _, p in fps for _, n in p})
    lines = ["\t(net_chains"]

    for name, members, term_a, term_b in spec:
        if isinstance(members, str):
            members = [n for n in nets if n.startswith(members)]

        lines.append("\t\t(net_chain")
        lines.append(f'\t\t\t(name "{name}")')
        lines.append("\t\t\t(members")

        for m in members:
            lines.append(f'\t\t\t\t(net "{m}")')

        lines.append("\t\t\t)")

        for term in (term_a, term_b):
            if term is None:
                continue

            kind, arg = term
            uuid = last_pad_of_net(fps, arg) if kind == "net" else fp_by_x(fps, arg)
            lines.append(f'\t\t\t(terminal_pad "{uuid}")')

        lines.append("\t\t)")

    lines.append("\t)")
    return "\n".join(lines) + "\n"


def insert_chains(text, block):
    end = text.rstrip().rfind(")")
    return text[:end] + block + text[end:]


# ---- cases -------------------------------------------------------------------------------

X = ("x", "min"), ("x", "max")

OWN = ["length_calculations", "time_calculations", "issue22536", "via_off_center",
       "drc_skew/drc_skew", "skew_within_diff_pairs_drc", "skew_group_matched_drc",
       "issue18045/test_proj", "issue23868/issue23868"]

APPEND = {
    "length_calculations": ["all", "same", "time", "dp"],
    "drc_skew/drc_skew": ["all", "same", "dp"],
    "issue22536": ["all", "same", "time"],
    "skew_within_diff_pairs_drc": ["all", "dp", "time"],
    "issue23868/issue23868": ["all"],
    "via_off_center": ["all"],
}

CHAINS = [
    ("net_chains/stub_length", [("SIG", ["/TRUNK_A", "/MID_B", "/TRUNK_C"],
                                 ("net", "/TRUNK_A"), ("net", "/TRUNK_C"))], ["chain", "chaintime"]),
    ("net_chains/stub_length_pad_to_die", [("SIG", ["/TRUNK_A", "/MID_B", "/TRUNK_C"],
                                            ("net", "/TRUNK_A"), ("net", "/TRUNK_C"))], ["chain"]),
    ("net_chains/stub_length_two_net", [("SIG", ["/A", "/B"], ("net", "/A"), ("net", "/B"))],
     ["chain"]),
    ("net_chains/stub_length#noterm", [("SIG", ["/TRUNK_A", "/MID_B", "/TRUNK_C"], None, None)],
     ["chain"]),
    ("net_chains/chain_topology_trunk", [("FastTopo", "/NET_", *X)], ["chain"]),
    ("net_chains/chain_topology_t_stub", [("FastTopo", "/NET_", *X)], ["chain", "chaintime"]),
    ("net_chains/chain_topology_cycle", [("LOOP", "/NET_", *X)], ["chain"]),
    ("net_chains/chain_topology_disconnected", [("DISC", "/NET_", *X)], ["chain"]),
    ("net_chains/chain_topology_missing_terminal", [("MISSING", "/NET_", X[0], None)], ["chain"]),
    ("net_chains/chain_length_daisy", [("DSY", "/NET_", *X)], ["chain", "chaintime"]),
    ("net_chains/chain_length_branched", [("BR", "/NET_", *X)], ["chain"]),
    ("net_chains/chain_bridging_3pad_1net", [("SIG", "/NET_", None, None)], ["chain"]),
    ("net_chains/chain_bridging_3pad_2net", [("SIG", "/NET_", None, None)], ["chain"]),
    ("net_chains/chain_bridging_3pad_3net", [("SIG", "/NET_", None, None)], ["chain"]),
    ("net_chains/chain_bridging_4pad", [("SIG", "/NET_", None, None)], ["chain"]),
    ("net_chains/length_with_chain", [("SIG", ["/CHAIN_NET"], None, None)], ["lenchain"]),
    ("net_chains/net_chain_total_length", [("TOT", "Net-", *X)], ["chain", "chaintime"]),
    ("net_chains/net_chain_trunk_delay", [("DLY", "/NET_", *X)], ["chain", "chaintime"]),
    ("net_chains/return_path_partial", [("PARTIAL", ["/CHAIN_PARTIAL"], None, None)], ["rp"]),
    ("net_chains/return_path_unshadowed", [("FULL", ["/CHAIN_FULL"], None, None)], ["rp"]),
    ("net_chains/return_path_zone_bbox", [("COVERED", ["/CHAIN_COVERED"], None, None),
                                          ("UNCOVERED", ["/CHAIN_UNCOVERED"], None, None)],
     ["rp"]),
    ("net_chains/return_path_dual_zone", [("RN", ["/CHAIN_RN"], None, None)],
     ["rp", "rpgnd", "rppwr", "rpgndw"]),
]


def run_case(case, rel, mode, chains_text):
    src = os.path.join(KICAD, QA + rel + ".kicad_pcb")
    stem = os.path.basename(rel)
    work = os.path.join(SCRATCH, re.sub(r"[/#:.]", "_", case))
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)

    text = open(src).read()

    if chains_text:
        text = insert_chains(text, chains_text)

    open(os.path.join(work, stem + ".kicad_pcb"), "w").write(text)

    for ext in (".kicad_pro", ".kicad_dru"):
        p = os.path.splitext(src)[0] + ext

        if os.path.exists(p) and (ext != ".kicad_dru" or not mode.startswith("replace")):
            shutil.copy(p, os.path.join(work, stem + ext))

    dru = os.path.join(work, stem + ".kicad_dru")

    if mode != "base":
        kind, variant = mode.split(":")
        rules = open(os.path.join(DATA, variant + ".dru")).read()

        if kind == "append":
            base = open(dru).read() if os.path.exists(dru) else "(version 1)\n"
            rules = base + "\n" + rules
        else:
            rules = "(version 1)\n" + rules

        open(dru, "w").write(rules)

    out = os.path.join(work, "report.json")
    env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(SCRATCH, "home"))
    subprocess.run(
        [CLI, "pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "-o", out,
         os.path.join(work, stem + ".kicad_pcb")],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    doc = json.load(open(out))
    entries = []

    for v in doc.get("violations", []):
        if v["type"] not in TYPES:
            continue

        severity = v["severity"] + ("/excluded" if v.get("excluded") else "")
        fields = [case, severity, v["type"], v["description"]]

        for it in v["items"]:
            fields += [it["uuid"], it["description"], repr(it["pos"]["x"]),
                       repr(it["pos"]["y"])]

        entries.append("\t".join(x.replace("\t", " ").replace("\n", " ") for x in fields))

    print(f"{case}: {len(entries)}")
    return entries if entries else [case + "\tnone"]


cases = []

for rel in OWN:
    cases.append((rel + "#base", rel, "base", None))

    for v in APPEND.get(rel, []):
        cases.append((f"{rel}#{v}", rel, "append:" + v, None))

for name, spec, variants in CHAINS:
    rel = name.split("#")[0]
    block = chains_block(QA + rel + ".kicad_pcb", spec)
    chains_file = re.sub(r"[/#]", "_", name) + ".chains"
    open(os.path.join(DATA, chains_file), "w").write(block)

    for v in variants:
        cases.append((f"{name}#{v}", rel, "replace:" + v, chains_file))

# A board with net chains in the file (issue25065): its own rules are none; chain rules added.
for v in ["chain", "chaintime"]:
    cases.append((f"issue25065/issue25065#{v}", "issue25065/issue25065", "replace:" + v, None))

lines = []

with open(os.path.join(DATA, "cases.tsv"), "w") as tsv:
    for case, rel, mode, chains_file in cases:
        tsv.write(f"{case}\t{rel}\t{mode}\t{chains_file or '-'}\n")
        chains_text = open(os.path.join(DATA, chains_file)).read() if chains_file else None
        lines += run_case(case, rel, mode, chains_text)

os.makedirs(os.path.dirname(OUT), exist_ok=True)

with open(OUT, "w") as out:
    out.write("\n".join(lines) + "\n")

print(f"{len(lines)} lines -> {OUT}")
