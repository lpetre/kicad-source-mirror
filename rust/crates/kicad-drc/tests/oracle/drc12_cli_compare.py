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

"""DRC-12: `kicad-rs pcb drc` against the reference `kicad-cli pcb drc` on the DRC corpus.

    python3 -I rust/crates/kicad-drc/tests/oracle/drc12_cli_compare.py ref  [BOARD...]
    python3 -I rust/crates/kicad-drc/tests/oracle/drc12_cli_compare.py cmp  [--rs KICAD_RS] [BOARD...]
    python3 -I rust/crates/kicad-drc/tests/oracle/drc12_cli_compare.py golden

The boards are those of tests/golden/drc_corpus.txt (every .kicad_pcb < 5 MB of qa/data/pcbnew
and demos without the large demos). Each tool runs on a copy of the board (with its
.kicad_pro / .kicad_dru / .kicad_prl) in a scratch directory, sequentially, once per case:

    pcb drc --format report --severity-all --units mm -o out.rpt  board.kicad_pcb
    pcb drc --format json   --severity-all --units mm -o out.json board.kicad_pcb

`ref` caches the reference outputs (report, stdout, exit code; the date line replaced by
<DATE>) under rust/target/drc12-cli/<path>.{rpt,json,out}. `cmp` runs kicad-rs and compares:

* exact: report, JSON and stdout byte for byte after normalising the date and the UUID of
  the board itself (`PCB` items, e.g. of `invalid_outline`: a random KIID per run in the
  reference); both tools print the same version and the same file name, nothing else is
  normalised;
* filtered: the entries of the error types whose provider is not ported (UNPORTED below) are
  removed from both reports (text: the entry blocks, with the section counts recomputed;
  JSON: the array entries), the not-run section / key of the Rust reports is removed, and the
  stdout lines `Found N ...` are compared with the filtered counts; then the texts must be
  equal, or equal as sorted entry lists per section ("same set, other order": the reference
  runs its providers on a thread pool).

`golden` writes tests/golden/drc12_cli.txt for the Rust replay in kicad-session
(tests/drc12_corpus.rs): per board the FNV-1a 64 of the filtered text report (exact and with
sorted sections) and the filtered counts.
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
CORPUS = os.path.join(HERE, "..", "golden", "drc_corpus.txt")
GOLDEN = os.path.join(HERE, "..", "golden", "drc12_cli.txt")
CACHE = os.path.join(KICAD, "rust", "target", "drc12-cli")
REF_CLI = "/home/user/kicad-oracle/bin/kicad-cli"
RS_CLI = os.path.join(KICAD, "rust", "target", "debug", "kicad-rs")

# Settings keys of the error codes of the providers that are not ported
# (kicad_drc::run_flow::UNPORTED_PROVIDERS) and of the providers whose packages are still
# running (creepage: DRC-33, length / skew / via count / net chain:
# DRC-31, diff pair: DRC-30 follow-ups); kicad-session/tests/drc12_corpus.rs filters the same keys.
UNPORTED = {
    "creepage", "duplicate_footprints",
    "extra_footprint", "footprint_filters_mismatch", "missing_footprint", "net_conflict",
    "footprint_symbol_field_mismatch", "footprint_symbol_mismatch",
    "net_chain_tuning_profiles", "via_stitch_overlap", "length_out_of_range",
    "net_chain_return_path", "net_chain_stub_length", "skew_out_of_range", "too_many_vias",
    "diff_pair_gap_out_of_range", "diff_pair_uncoupled_length_too_long",
}
NOT_RUN_HEADING = "** Checks not run (not ported yet) **"
FNV_OFFSET = 0xCBF29CE484222325


def fnv1a64(data):
    h = FNV_OFFSET
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def boards():
    seen = []
    with open(CORPUS) as f:
        for line in f:
            rel = line.split("\t", 1)[0]
            if not seen or seen[-1] != rel:
                seen.append(rel)
    return seen


def run_tool(cli, rel, fmt, env):
    """(output text or None, stdout, exit code) of one run on a copy."""
    src = os.path.join(KICAD, rel)
    base = os.path.splitext(src)[0]

    with tempfile.TemporaryDirectory(prefix="drc12-") as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru", ".kicad_prl"):
            if os.path.exists(base + ext):
                shutil.copy(base + ext, os.path.join(tmp, os.path.basename(base) + ext))

        board = os.path.join(tmp, os.path.basename(src))
        out = os.path.join(tmp, "out." + ("rpt" if fmt == "report" else "json"))

        try:
            p = subprocess.run(["nice", "-n", "10", cli, "pcb", "drc", "--format", fmt,
                                "--severity-all", "--units", "mm", "-o", out, board],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env,
                               timeout=900, cwd=tmp)
        except subprocess.TimeoutExpired:
            return None, "", -1

        text = None
        if os.path.exists(out):
            with open(out, encoding="utf-8", errors="surrogateescape") as f:
                text = normalise(f.read())

        stdout = p.stdout.decode("utf-8", "surrogateescape").replace(tmp + "/", "")
        return text, stdout, p.returncode


def normalise(text):
    text = re.sub(r"^\*\* Created on .* \*\*$", "** Created on <DATE> **", text, flags=re.M)
    return re.sub(r'^    "date": ".*",$', '    "date": "<DATE>",', text, flags=re.M)


# The JSON item of the board itself (BOARD::m_Uuid is a random KIID per run in C++).
BOARD_ITEM_UUID = re.compile(r'("description": "PCB",\n\s+"pos": \{[^}]*\},\n\s+"uuid": ")'
                             r'[0-9a-f-]{36}"')


def normalise_board_uuid(text):
    return BOARD_ITEM_UUID.sub(r'\1<BOARD>"', text) if text is not None else None


def cache_path(rel, ext):
    return os.path.join(CACHE, rel + "." + ext)


def ref(selected):
    env = dict(os.environ, KICAD_ORACLE_HOME=tempfile.mkdtemp(prefix="drc12-home-"))
    os.makedirs(CACHE, exist_ok=True)

    for rel in selected:
        if os.path.exists(cache_path(rel, "out")):
            continue

        rpt, out_r, code_r = run_tool(REF_CLI, rel, "report", env)
        js, out_j, code_j = run_tool(REF_CLI, rel, "json", env)
        os.makedirs(os.path.dirname(cache_path(rel, "out")), exist_ok=True)

        if rpt is not None:
            with open(cache_path(rel, "rpt"), "w", encoding="utf-8", errors="surrogateescape") as f:
                f.write(rpt)
        if js is not None:
            with open(cache_path(rel, "json"), "w", encoding="utf-8", errors="surrogateescape") as f:
                f.write(js)
        with open(cache_path(rel, "out"), "w", encoding="utf-8", errors="surrogateescape") as f:
            f.write(json.dumps({"report": [out_r, code_r], "json": [out_j, code_j]}))
        print(rel, code_r, code_j, flush=True)


def read_cache(rel, ext):
    p = cache_path(rel, ext)
    if not os.path.exists(p):
        return None
    with open(p, encoding="utf-8", errors="surrogateescape") as f:
        return f.read()


def parse_text(text):
    """[(header line, [entry block])], the head lines, the tail lines."""
    lines = text.split("\n")
    head, sections, tail = [], [], []
    cur = None
    for line in lines:
        if tail:
            tail.append(line)
            continue
        if line.startswith("** Found "):
            cur = (line, [])
            sections.append(cur)
            continue
        if cur is None:
            head.append(line)
            continue
        if line.startswith("** ") and not line.startswith("** Found "):
            cur = None
            tail.append(line)
            continue
        if line.startswith("["):
            cur[1].append(line + "\n")
        elif line.startswith("    ") and cur[1]:
            cur[1][-1] += line + "\n"
    return head, sections, tail


def normalise_block(block):
    """Known reference effect, applied to both sides: the item order of `connection_width`
    (C++ std::set of item pointers: address order)."""
    lines = [l for l in block.split("\n") if l]
    if block.startswith("[connection_width]"):
        lines = lines[:1] + sorted(l for l in lines[1:] if l.startswith("    @")) + \
            [l for l in lines[1:] if not l.startswith("    @")]
    return "\n".join(lines) + "\n"


def at_limit(entries, type_of, placeholder):
    """drc.md 9.4 (3): a type with ERROR_LIMIT (199) or EXTENDED_ERROR_LIMIT (499) entries
    is compared by count only (which entries the reference keeps depends on its thread
    scheduling). The entries are replaced by one placeholder at the first one."""
    counts = {}
    for e in entries:
        counts[type_of(e)] = counts.get(type_of(e), 0) + 1
    out, done = [], set()
    for e in entries:
        t = type_of(e)
        if counts[t] in (199, 499):
            if t not in done:
                done.add(t)
                out.append(placeholder(t, counts[t]))
            continue
        out.append(e)
    return out


def entry_type(block):
    return block[1:block.index("]")]


def filter_text(text, sort=False):
    """The text report without unported entries / the not-run section; counts recomputed.
    Returns (text, counts)."""
    # drop the not-run section
    if NOT_RUN_HEADING in text:
        a = text.index("\n" + NOT_RUN_HEADING)
        b = text.index("\n** End of Report **")
        text = text[:a] + text[b:]

    head, sections, tail = parse_text(text)
    out = "\n".join(head) + "\n"
    counts = []
    for header, blocks in sections:
        kept = [normalise_block(b) for b in blocks if entry_type(b) not in UNPORTED]
        kept = at_limit(kept, entry_type, lambda t, n: "[%s]: %d entries (error limit)\n" % (t, n))
        if sort:
            kept = sorted(kept)
        counts.append(len(kept))
        out += re.sub(r"Found \d+ ", "Found %d " % len(kept), header) + "\n"
        out += "".join(kept) + "\n"
    out += "\n".join(tail)
    return out, counts


def filter_json(text, rel):
    with open(os.path.join(KICAD, rel), encoding="utf-8", errors="surrogateescape") as f:
        board_text = f.read()
    doc = json.loads(text)
    doc.pop("checks_not_run", None)
    for key in ("violations", "unconnected_items", "schematic_parity"):
        doc[key] = [v for v in doc.get(key, []) if v["type"] not in UNPORTED]
        doc[key] = at_limit(doc[key], lambda v: v["type"],
                            lambda t, n: {"type": t, "count": n})
        for v in doc[key]:
            if "items" not in v:
                continue
            # drc.md 9.4 (4): items without a UUID in the file get a random one in C++ (the
            # board itself, `PCB`, too)
            for i in v["items"]:
                if i["uuid"] not in board_text:
                    i["uuid"] = "<generated>"
            if v["type"] == "connection_width":
                v["items"].sort(key=lambda i: json.dumps(i, sort_keys=True))
    return doc


def sorted_json(doc):
    doc = dict(doc)
    for key in ("violations", "unconnected_items", "schematic_parity"):
        doc[key] = sorted(doc[key], key=lambda v: json.dumps(v, sort_keys=True))
    return doc


def stdout_counts(stdout):
    return [int(x) for x in re.findall(r"^Found (\d+) ", stdout, flags=re.M)]


def strip_not_run(stdout):
    return "".join(l for l in stdout.splitlines(True) if not l.startswith("DRC check '"))


def compare(selected, rs):
    env = dict(os.environ)
    score = {"boards": 0, "exact": 0, "filtered": 0, "set": 0, "diff": 0, "rs_failed": 0,
             "ref_failed": 0}
    per_type = {}
    for rel in selected:
        ref_rpt, ref_js, ref_out = read_cache(rel, "rpt"), read_cache(rel, "json"), read_cache(rel, "out")
        if ref_out is None:
            print("NOREF", rel)
            continue
        ref_out = json.loads(ref_out)
        if ref_rpt is None or ref_js is None:
            score["ref_failed"] += 1
            print("REF-FAILED", rel)
            continue

        rs_rpt, rs_out_r, rs_code_r = run_tool(rs, rel, "report", env)
        rs_js, rs_out_j, rs_code_j = run_tool(rs, rel, "json", env)
        ref_js, rs_js = normalise_board_uuid(ref_js), normalise_board_uuid(rs_js)
        if rs_rpt is None or rs_js is None:
            score["rs_failed"] += 1
            print("RS-FAILED", rel, rs_code_r, rs_out_r.strip()[:200])
            continue

        score["boards"] += 1
        exact = (rs_rpt == ref_rpt and rs_js == ref_js and rs_out_r == ref_out["report"][0]
                 and rs_code_r == ref_out["report"][1])
        if exact:
            score["exact"] += 1
            print("EXACT", rel)
            continue

        ft_ref, c_ref = filter_text(ref_rpt)
        ft_rs, c_rs = filter_text(rs_rpt)
        fj_ref, fj_rs = filter_json(ref_js, rel), filter_json(rs_js, rel)
        so_ok = (stdout_counts(strip_not_run(rs_out_r)) == stdout_counts(rs_out_r)[:2]
                 and rs_code_r == ref_out["report"][1])
        # the reference stdout counts include the unported entries: compare the Rust stdout
        # counts with the Rust report and the reference stdout with the reference report.
        so_ok = so_ok and stdout_counts(ref_out["report"][0])[:2] == \
            filter_text_counts_all(ref_rpt)[:2] and stdout_counts(rs_out_r)[:2] == \
            filter_text_counts_all(rs_rpt)[:2]
        if ft_ref == ft_rs and fj_ref == fj_rs and so_ok:
            score["filtered"] += 1
            print("FILTERED", rel)
            continue
        st_ref, _ = filter_text(ref_rpt, True)
        st_rs, _ = filter_text(rs_rpt, True)
        if st_ref == st_rs and sorted_json(fj_ref) == sorted_json(fj_rs) and so_ok:
            score["set"] += 1
            print("SET", rel)
            continue
        score["diff"] += 1
        print("DIFF", rel, "stdout-ok" if so_ok else "stdout/exit differ")
        # per type multiset diff of the JSON entries
        for key in ("violations", "unconnected_items"):
            a = [json.dumps(v, sort_keys=True) for v in fj_ref[key]]
            b = [json.dumps(v, sort_keys=True) for v in fj_rs[key]]
            for v in a:
                if v in b:
                    b.remove(v)
                else:
                    t = json.loads(v)["type"]
                    per_type.setdefault(t, [0, 0])[0] += 1
                    print("   -", v[:400])
            for v in b:
                t = json.loads(v)["type"]
                per_type.setdefault(t, [0, 0])[1] += 1
                print("   +", v[:400])
        if fj_ref.get("ignored_checks") != fj_rs.get("ignored_checks"):
            print("   ignored_checks differ")
    print(score)
    print("missing/extra per type:", per_type)


def filter_text_counts_all(text):
    _, sections, _ = parse_text(text)
    return [len(b) for _, b in sections]


def golden(selected):
    lines = []
    for rel in selected:
        rpt = read_cache(rel, "rpt")
        if rpt is None:
            lines.append("%s\tref_failed" % rel)
            continue
        exact, counts = filter_text(rpt)
        srt, _ = filter_text(rpt, True)
        lines.append("%s\t%016x\t%016x\t%s" % (rel, fnv1a64(exact.encode("utf-8", "surrogateescape")),
                                             fnv1a64(srt.encode("utf-8", "surrogateescape")),
                                             ",".join(map(str, counts))))
    with open(GOLDEN, "w") as f:
        f.write("\n".join(lines) + "\n")


def main():
    args = sys.argv[1:]
    mode = args.pop(0)
    rs = RS_CLI
    if args[:1] == ["--rs"]:
        rs = args[1]
        args = args[2:]
    selected = args or boards()
    if mode == "ref":
        ref(selected)
    elif mode == "cmp":
        compare(selected, rs)
    elif mode == "golden":
        golden(boards())


if __name__ == "__main__":
    main()
