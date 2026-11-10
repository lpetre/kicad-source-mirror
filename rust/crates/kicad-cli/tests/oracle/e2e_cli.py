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

"""End-to-end differential test of kicad-rs against the reference kicad-cli (WIRE-1).

For every corpus board under 300 KB (the board list of
kicad-session/tests/golden/upgrade_corpus.txt) and every command of COMMANDS, run the
reference `kicad-cli` and `kicad-rs` on COPIES of the board (+ .kicad_pro / .kicad_dru) in a
scratch directory, then compare exit code, stdout, stderr (kicad-rs only lines that are not
`kicad-rs:` diagnostics) and every produced file byte for byte after the normalisations of
`normalise()` (dates, the scratch path).

Reference results are cached under rust/target/e2e/ref/<board>/<command>/ (delete a directory
to refresh it). kicad-rs runs with KICAD_RS_DATE / KICAD_RS_UUID_SEED fixed.

Usage (from rust/):
  cargo build -p kicad-cli
  python3 crates/kicad-cli/tests/oracle/e2e_cli.py [--filter SUBSTR] [--commands a,b]
          [--write-golden] [--show-diff]

--write-golden writes the compact summary crates/kicad-cli/tests/golden/e2e_cli.txt
(board, command, status, fnv1a64 of the normalised reference outputs), which the Rust replay
test tests/e2e_cli.rs checks without the oracle.
"""

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys

RUST = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
KICAD = os.path.dirname(RUST)
REF_CLI = os.environ.get("KICAD_ORACLE_CLI", "/home/user/kicad-oracle/bin/kicad-cli")
RS_CLI = os.path.join(RUST, "target", "debug", "kicad-rs")
CACHE = os.path.join(RUST, "target", "e2e", "ref")
WORK = os.path.join(RUST, "target", "e2e", "work")
CORPUS_LIST = os.path.join(RUST, "crates", "kicad-session", "tests", "golden", "upgrade_corpus.txt")
GOLDEN = os.path.join(RUST, "crates", "kicad-cli", "tests", "golden", "e2e_cli.txt")
FIXED_DATE = "2026-01-02T03:04:05"

# name -> (arguments before the input file; output path inside the work dir or None)
COMMANDS = {
    "upgrade": (["pcb", "upgrade", "--force"], None),
    "drc-report": (["pcb", "drc", "--format", "report", "-o", "out/drc.rpt"], "out"),
    "drc-json": (["pcb", "drc", "--format", "json", "--severity-all", "-o", "out/drc.json"], "out"),
    "gerbers": (["pcb", "export", "gerbers", "-o", "out/"], "out"),
    "drill-excellon": (["pcb", "export", "drill", "-o", "out/"], "out"),
    "drill-gerber": (["pcb", "export", "drill", "--format", "gerber", "-o", "out/"], "out"),
    "svg": (["pcb", "export", "svg", "--mode-multi", "-l", "F.Cu,B.Cu,F.Silkscreen,F.Mask,Edge.Cuts", "-o", "out/"], "out"),
}

DATE_RES = [
    # ISO 8601 with optional offset (DRC report / JSON, %TF.CreationDate, Excellon, gbrjob)
    (re.compile(rb"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}([+-]\d{2}:?\d{2}|Z)?"), b"<DATE>"),
    # G04 Created by KiCad (PCBNEW ...) date 2026-11-10 05:24:54*
    (re.compile(rb"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}"), b"<DATE>"),
]


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h


def normalise(data, work=None):
    """Dates and the absolute work directory (both tools run in a directory of their own)."""
    for rx, rep in DATE_RES:
        data = rx.sub(rep, data)
    if work:
        data = data.replace(work.encode(), b"<WORK>")
    return data


def board_list():
    out = []
    with open(CORPUS_LIST) as f:
        for line in f:
            if line.startswith("#") or not line.strip():
                continue
            out.append(line.split("\t")[0])
    return out


def prepare(board, work):
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    src = os.path.join(KICAD, board)
    base = os.path.splitext(src)[0]
    for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
        if os.path.exists(base + ext):
            shutil.copy(base + ext, work)
    return os.path.basename(src)


def snapshot(work, name, out_dir):
    """The produced files: the (possibly rewritten) board and everything under out/."""
    files = {}
    with open(os.path.join(work, name), "rb") as f:
        files["<board>"] = f.read()
    if out_dir:
        root = os.path.join(work, out_dir)
        for dirpath, _, names in os.walk(root):
            for n in sorted(names):
                p = os.path.join(dirpath, n)
                with open(p, "rb") as f:
                    files[os.path.relpath(p, work)] = f.read()
    return files


def run(tool, board, cmd, work, env):
    args, out_dir = COMMANDS[cmd]
    name = prepare(board, work)
    try:
        p = subprocess.run([tool] + args + [name], cwd=work, env=env, capture_output=True, timeout=600)
        code, stdout, stderr = p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        code, stdout, stderr = "timeout", b"", b""
    files = snapshot(work, name, out_dir)
    stderr = b"".join(l for l in stderr.splitlines(True) if b"api.v1.schema.json" not in l)
    return {"exit": str(code).encode(), "stdout": stdout, "stderr": stderr, **{"file:" + k: v for k, v in files.items()}}


def ref_result(board, cmd):
    cache = os.path.join(CACHE, board.replace("/", "__"), cmd)
    if os.path.isdir(cache):
        res = {}
        for dirpath, _, names in os.walk(cache):
            for n in names:
                p = os.path.join(dirpath, n)
                with open(p, "rb") as f:
                    res[os.path.relpath(p, cache).replace("__SLASH__", "/")] = f.read()
        return {k: normalise(v) for k, v in res.items()}
    work = os.path.join(WORK, "ref")
    env = dict(os.environ)
    env["KICAD_ORACLE_HOME"] = os.path.join(RUST, "target", "e2e", "home")
    res = run(REF_CLI, board, cmd, work, env)
    # cached raw except for the work directory; dates are normalised at compare time
    res = {k: v.replace(work.encode(), b"<WORK>") for k, v in res.items()}
    tmp = cache + ".tmp"
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    for k, v in res.items():
        p = os.path.join(tmp, k.replace("/", "__SLASH__"))
        with open(p, "wb") as f:
            f.write(v)
    os.rename(tmp, cache)
    return {k: normalise(v) for k, v in res.items()}


def rs_result(board, cmd):
    work = os.path.join(WORK, "rs")
    env = dict(os.environ)
    env["KICAD_RS_DATE"] = FIXED_DATE
    env["KICAD_RS_UUID_SEED"] = "1"
    res = run(RS_CLI, board, cmd, work, env)
    return {k: normalise(v, work) for k, v in res.items()}


def not_runnable_reason(rs):
    for line in rs.get("stderr", b"").decode(errors="replace").splitlines():
        if line.startswith("kicad-rs: not yet ported") or line.startswith("kicad-rs: internal error"):
            return line[len("kicad-rs: "):]
    return None


def compare(ref, rs):
    """'identical' or the first differing key."""
    # kicad-rs only warnings (font fallbacks) are not part of the comparison
    rs = dict(rs)
    rs["stderr"] = b"".join(l for l in rs["stderr"].splitlines(True) if not l.startswith(b"Warning: text uses font"))
    keys = sorted(set(ref) | set(rs))
    for k in keys:
        if ref.get(k) != rs.get(k):
            return k
    return None


def golden_hash(ref):
    h = hashlib.sha256()
    for k in sorted(ref):
        h.update(k.encode() + b"\0" + ref[k] + b"\0")
    return fnv1a64(h.digest())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--filter", default="")
    ap.add_argument("--commands", default=",".join(COMMANDS))
    ap.add_argument("--write-golden", action="store_true")
    ap.add_argument("--show-diff", action="store_true")
    a = ap.parse_args()

    if not os.path.exists(REF_CLI) or not os.path.exists(RS_CLI):
        print("reference kicad-cli or kicad-rs missing; skipped")
        return 0

    cmds = [c for c in a.commands.split(",") if c]
    boards = [b for b in board_list() if a.filter in b]
    score = {c: {"identical": 0, "differing": 0, "not runnable": 0} for c in cmds}
    lines = []
    reasons = {}

    for board in boards:
        for cmd in cmds:
            ref = ref_result(board, cmd)
            rs = rs_result(board, cmd)
            reason = not_runnable_reason(rs)
            if reason is not None:
                status = "not-runnable"
                score[cmd]["not runnable"] += 1
                reasons.setdefault((cmd, reason), []).append(board)
                detail = reason
            else:
                diff = compare(ref, rs)
                if diff is None:
                    status = "identical"
                    score[cmd]["identical"] += 1
                    detail = ""
                else:
                    status = "differing"
                    score[cmd]["differing"] += 1
                    detail = diff
                    if a.show_diff:
                        print("== %s %s: %s" % (board, cmd, diff))
                        r = ref.get(diff, b"<missing>").decode(errors="replace").splitlines()
                        s = rs.get(diff, b"<missing>").decode(errors="replace").splitlines()
                        for i, (x, y) in enumerate(zip(r + ["<eof>"] * len(s), s + ["<eof>"] * len(r))):
                            if x != y:
                                print("   line %d\n   ref: %s\n   rs:  %s" % (i + 1, x[:200], y[:200]))
                                break
            lines.append("%s\t%s\t%s\t%s\t%s" % (board, cmd, status, golden_hash(ref), detail))
            print("%-60s %-15s %s %s" % (board, cmd, status, detail), flush=True)

    print("\nScoreboard (%d boards)" % len(boards))
    for c in cmds:
        s = score[c]
        print("  %-15s identical %4d  differing %4d  not runnable %4d" % (c, s["identical"], s["differing"], s["not runnable"]))
    for (c, r), bs in sorted(reasons.items()):
        print("  not runnable %s: %d x %s (e.g. %s)" % (c, len(bs), r, bs[0]))

    if a.write_golden:
        with open(GOLDEN, "w") as f:
            f.write("# kicad-rs vs reference kicad-cli, end to end (WIRE-1). Generated by\n")
            f.write("# `python3 crates/kicad-cli/tests/oracle/e2e_cli.py --write-golden` (from rust/).\n")
            f.write("# board\tcommand\tstatus\tfnv1a64(normalised reference outputs)\tdetail\n")
            for l in lines:
                f.write(l + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
