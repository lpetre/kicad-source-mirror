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

"""FIX-FAB-2: the fabrication option matrix of REV-FAB (rust/docs/design/review-fab.md) as a
committed scoreboard of hashed goldens.

For every case (board, optional mutation of a copy of it, `pcb export gerbers|drill` options)
the reference `kicad-cli` and `kicad-rs` run on COPIES of the board (+ .kicad_pro / .kicad_dru)
in a scratch directory with `-o out/`; exit code, stdout, stderr (without the reference's
`api.v1.schema.json` line and kicad-rs' `Warning: text uses font` lines) and every file under
`out/` are normalised (dates, wxLog time stamps, the scratch path: the rules of
`tests/oracle/e2e_cli.py`) and hashed with FNV-1a 64 (printed as its low 32 bits, 8 hex
digits). `cases.txt` records per case the reference hashes and the status of kicad-rs at
generation time; `tests/fab_matrix.rs` replays kicad-rs on every `identical` case.

Case groups (option sets from docs/design/review-fab/tools/*.txt):
  gerbers   gerber_sets.txt x basic_test, issue6945, ecc83-pp
  dnp       dnp_sets.txt x copies of basic_test / ecc83-pp with 8 footprints marked DNP
  stored    stored_variants.txt (mutated pcbplotparams) + --board-plot-params x basic_test,
            issue6945 (fmt0 = stored HPGL, F-01)
  define    d_sets.txt x issue22267, basic_test (F-02)
  variant   --variant sets x variant_test, issue24735 (F-05)
  drillx    the 256 Excellon combinations of drill_matrix.py full on drill_features (one
            hash over all files per case)
  drill     the other drill sets of drill_matrix.py + report paths (F-04) x drill_features,
            basic_test

Mutations (`key=value` replaces / adds a pcbplotparams entry, `@dnp=N` marks every third
footprint DNP up to N) are those of review-fab/tools/mutate.py; the Rust replay ports them and
checks the mutated board against the hash recorded here.

Usage (from rust/):  cargo build -p kicad-cli
                     python3 crates/kicad-cli/tests/golden/fab_matrix/generate.py [--filter S]
Reference results are cached under rust/target/fab_matrix/ref/ (delete to refresh).
"""

import argparse
import itertools
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
KICAD = os.path.dirname(RUST)
TOOLS = os.path.join(RUST, "docs", "design", "review-fab", "tools")
REF_CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
RS_CLI = os.path.join(RUST, "target", "debug", "kicad-rs")
SCRATCH = os.path.join(RUST, "target", "fab_matrix")
GOLDEN = os.path.join(HERE, "cases.txt")
FIXED_DATE = "2026-01-02T03:04:05"

BASIC = "qa/data/cli/basic_test/basic_test.kicad_pcb"
I6945 = "qa/data/pcbnew/issue6945.kicad_pcb"
ECC83 = "demos/ecc83/ecc83-pp.kicad_pcb"
I22267 = "qa/data/pcbnew/issue22267.kicad_pcb"
VARIANT = "qa/data/pcbnew/variant_test/variant_test.kicad_pcb"
I24735 = "qa/data/pcbnew/issue24735/issue24735.kicad_pcb"
DRILLF = "rust/crates/kicad-fab/tests/data/excellon_corpus/drill_features.kicad_pcb"


def read_sets(name, sep="|"):
    out = []
    for line in open(os.path.join(TOOLS, name), encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip():
            continue
        if sep == "|":
            tag, opts = line.split("|", 1)
            out.append((tag, opts.split()))
        else:
            tag, _, rest = line.partition(" ")
            out.append((tag, rest.split()))
    return out


def cases():
    """(group, tag, kind, board, mutation tokens, options) for every case."""
    out = []
    for board in (BASIC, I6945, ECC83):
        for tag, opts in read_sets("gerber_sets.txt"):
            out.append(("gerbers", tag, "gerbers", board, [], opts))
    for board in (BASIC, ECC83):
        for tag, opts in read_sets("dnp_sets.txt"):
            out.append(("dnp", tag, "gerbers", board, ["@dnp=8"], opts))
    for board in (BASIC, I6945):
        for tag, muts in read_sets("stored_variants.txt", sep=" "):
            out.append(("stored", tag, "gerbers", board, muts, ["--board-plot-params"]))
    for board in (I22267, BASIC):
        for tag, opts in read_sets("d_sets.txt"):
            out.append(("define", tag, "gerbers", board, [], opts))
    for board, names in ((VARIANT, ("Variant A", "variant a")), (I24735, ("var1", "var2"))):
        v1, v2 = names
        for tag, opts in (
            ("default", []),
            ("v1", ["--variant", v1]),
            ("v1dnp", ["--variant", v1, "--cdnp", "--sdnp", "--sp"]),
            ("v1hdnp", ["--variant", v1, "--hdnp"]),
            ("v2", ["--variant", v2, "--cdnp"]),
            ("vnone", ["--variant", "Nope", "--cdnp"]),
            ("v12", ["--variant", v1, "--variant", v2, "--cdnp"]),
        ):
            out.append(("variant", tag, "gerbers", board, [], opts))
    for z, u, o, m, h, s, ov in itertools.product(
        ("decimal", "suppressleading", "suppresstrailing", "keep"), ("mm", "in"),
        ("absolute", "plot"), (0, 1), (0, 1), (0, 1), ("alternate", "route")
    ):
        opts = ["--excellon-zeros-format", z, "-u", u, "--drill-origin", o, "--excellon-oval-format", ov]
        if m:
            opts.append("--excellon-mirror-y")
        if h:
            opts.append("--excellon-min-header")
        if s:
            opts.append("--excellon-separate-th")
        tag = "%s_%s_%s_m%d_h%d_s%d_%s" % (z[:8], u, o[:3], m, h, s, ov[:3])
        out.append(("drillx", tag, "drill", DRILLF, [], opts))
    other = []
    for p, o in itertools.product(("5", "6"), ("absolute", "plot")):
        other.append(("gbr_p%s_%s" % (p, o[:3]), ["--format", "gerber", "--gerber-precision", p, "--drill-origin", o]))
        other.append(("gbr_p%s_%s_tent" % (p, o[:3]),
                      ["--format", "gerber", "--gerber-precision", p, "--drill-origin", o, "--generate-tenting"]))
    other += [
        ("gbr_sep", ["--format", "gerber", "--excellon-separate-th", "--excellon-mirror-y", "-u", "in"]),
        ("exc_tent", ["--generate-tenting"]),
        ("exc_report", ["--generate-report"]),
        ("exc_report_path", ["--generate-report", "--report-path", "out/my report.txt"]),
        # F-04: the directory of the report does not exist
        ("exc_report_nodir", ["--generate-report", "--report-path", "out/rep/r.txt"]),
        ("gbr_report_nodir", ["--format", "gerber", "--generate-report", "--report-path", "out/a/b/r.txt"]),
        ("exc_report_noflag", ["--report-path", "out/r.txt"]),
        ("exc_report_sep", ["--generate-report", "--excellon-separate-th", "-u", "in"]),
        ("gbr_report", ["--format", "gerber", "--generate-report"]),
        ("map_gerberx2", ["--generate-map", "--map-format", "gerberx2"]),
        ("map_svg", ["--generate-map", "--map-format", "svg"]),
        ("map_pdf", ["--generate-map", "--map-format", "pdf"]),
        ("gbr_map_gbr", ["--format", "gerber", "--generate-map", "--map-format", "gerberx2"]),
        ("bad_zeros", ["--excellon-zeros-format", "nope"]),
        ("bad_units", ["-u", "furlong"]),
        ("bad_prec", ["--format", "gerber", "--gerber-precision", "7"]),
    ]
    for board in (DRILLF, BASIC):
        for tag, opts in other:
            out.append(("drill", tag, "drill", board, [], opts))
    return out


# ---- mutate.py (review-fab/tools), as a function --------------------------------------------

def block_end(t, start):
    depth = 0
    i = start
    while True:
        c = t[i]
        if c == '"':
            i += 1
            while t[i] != '"':
                if t[i] == "\\":
                    i += 1
                i += 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
        i += 1


def mutate(t, tokens):
    start = re.search(r"\(pcbplotparams\b", t).start()
    end = block_end(t, start)
    block = t[start:end + 1]
    dnp = 0
    for kv in tokens:
        if kv.startswith("@dnp="):
            dnp = int(kv[5:])
            continue
        k, v = kv.split("=", 1)
        pat = re.compile(r"\(" + re.escape(k) + r"\s[^()]*\)")
        if pat.search(block):
            block = pat.sub("(%s %s)" % (k, v), block, count=1)
        else:
            block = block[:-1] + "\t(%s %s)\n\t\t)" % (k, v)
    t = t[:start] + block + t[end + 1:]
    if dnp:
        parts = re.split(r"(?m)^(\t\(footprint )", t)
        out = [parts[0]]
        n = 0
        idx = 0
        for j in range(1, len(parts), 2):
            body = parts[j + 1]
            if idx % 3 == 0 and n < dnp:
                if re.search(r"(?m)^\t\t\(attr [^)]*\)", body):
                    body = re.sub(r"(?m)^(\t\t\(attr [^)]*)\)",
                                  lambda mm: mm.group(1) + (" dnp)" if " dnp" not in mm.group(1) else ")"),
                                  body, count=1)
                else:
                    body = re.sub(r"(?m)^(\t\t\(property )", "\t\t(attr dnp)\n\\1", body, count=1)
                n += 1
            idx += 1
            out.append(parts[j])
            out.append(body)
        t = "".join(out)
    return t


# ---- running and hashing -------------------------------------------------------------------

DATE_RES = [
    (re.compile(rb"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}([+-]\d{2}:?\d{2}|Z)?"), b"<DATE>"),
    (re.compile(rb"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}"), b"<DATE>"),
    (re.compile(rb"(?m)^\d{2}:\d{2}:\d{2}: "), b"<TIME>: "),
]


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def h8(data):
    return "%08x" % (fnv1a64(data) & 0xFFFFFFFF)


def normalise(data, work):
    for rx, rep in DATE_RES:
        data = rx.sub(rep, data)
    return data.replace(work.encode(), b"<WORK>")


def prepare(board, tokens, work):
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    src = os.path.join(KICAD, board)
    base = os.path.splitext(src)[0]
    name = os.path.basename(src)
    data = open(src, "rb").read()
    if tokens:
        data = mutate(data.decode("utf-8"), tokens).encode("utf-8")
    open(os.path.join(work, name), "wb").write(data)
    for ext in (".kicad_pro", ".kicad_dru"):
        if os.path.exists(base + ext):
            shutil.copy(base + ext, work)
    return name, h8(data)


def run(tool, case, work, env):
    _, _, kind, board, tokens, opts = case
    name, board_hash = prepare(board, tokens, work)
    args = [tool, "pcb", "export", kind] + opts + ["-o", "out/", name]
    p = subprocess.run(args, cwd=work, env=env, capture_output=True, timeout=900)
    stderr = b"".join(l for l in p.stderr.splitlines(True)
                      if b"api.v1.schema.json" not in l and not l.startswith(b"Warning: text uses font"))
    res = {"exit": str(p.returncode).encode(), "stdout": p.stdout, "stderr": stderr}
    root = os.path.join(work, "out")
    for dirpath, _, names in os.walk(root):
        for n in names:
            f = os.path.join(dirpath, n)
            res["file:" + os.path.relpath(f, work)] = open(f, "rb").read()
    return board_hash, {k: normalise(v, work) for k, v in res.items()}


def hashes(case, res):
    """key -> 8 hex digits; the drill factorial as one hash over all keys."""
    if case[0] == "drillx":
        data = b"".join(k.encode() + b"\0" + res[k] + b"\0" for k in sorted(res))
        return {"*": h8(data)}
    return {k: h8(v) for k, v in res.items()}


def ref_hashes(case):
    key = "%s__%s__%s" % (case[0], os.path.basename(case[3]), case[1])
    cache = os.path.join(SCRATCH, "ref", key + ".txt")
    if os.path.exists(cache):
        lines = open(cache, encoding="utf-8").read().splitlines()
        return lines[0], dict(l.split("\t", 1)[::-1] for l in lines[1:])
    env = dict(os.environ)
    env["KICAD_ORACLE_HOME"] = os.path.join(SCRATCH, "home")
    board_hash, res = run(REF_CLI, case, os.path.join(SCRATCH, "work", "ref"), env)
    hs = hashes(case, res)
    os.makedirs(os.path.dirname(cache), exist_ok=True)
    with open(cache + ".tmp", "w", encoding="utf-8") as f:
        f.write(board_hash + "\n" + "".join("%s\t%s\n" % (v, k) for k, v in sorted(hs.items())))
    os.rename(cache + ".tmp", cache)
    return board_hash, hs


def status(ref, rs, rs_res):
    for line in rs_res.get("stderr", b"").decode(errors="replace").splitlines():
        if line.startswith("kicad-rs: "):
            return "not-runnable:" + line[len("kicad-rs: "):].replace("\t", " ")[:120]
    for k in sorted(set(ref) | set(rs)):
        if ref.get(k) != rs.get(k):
            return "differing:" + k
    return "identical"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--filter", default="")
    args = ap.parse_args()
    env = dict(os.environ)
    env["KICAD_RS_DATE"] = FIXED_DATE
    env["KICAD_RS_UUID_SEED"] = "1"
    lines = []
    counts = {}
    for case in cases():
        group, tag, kind, board, tokens, opts = case
        if args.filter and args.filter not in "%s %s %s" % (group, tag, board):
            continue
        board_hash, ref = ref_hashes(case)
        rs_board_hash, rs_res = run(RS_CLI, case, os.path.join(SCRATCH, "work", "rs"), env)
        assert rs_board_hash == board_hash
        rs = hashes(case, rs_res)
        st = status(ref, rs, rs_res)
        counts.setdefault(group, {}).setdefault(st.split(":")[0], 0)
        counts[group][st.split(":")[0]] += 1
        if st != "identical":
            print("%s %s %s: %s" % (group, tag, board, st))
        lines.append("\t".join(["C", group, tag, kind, board, " ".join(tokens) or "-", board_hash, st] + opts))
        lines += ["H\t%s\t%s" % (v, k) for k, v in sorted(ref.items())]
    for g, c in counts.items():
        print(g, " ".join("%s=%d" % kv for kv in sorted(c.items())))
    if args.filter:
        return
    with open(GOLDEN, "w", encoding="utf-8") as f:
        f.write("# FIX-FAB-2 fabrication option matrix: reference kicad-cli hashes (FNV-1a 64, low 32 bits\n"
                "# of the normalised outputs). Generated by generate.py; replayed by tests/fab_matrix.rs.\n"
                "# C group tag kind board mutation board-hash status options...\n"
                "# H hash key\n")
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    sys.exit(main())
