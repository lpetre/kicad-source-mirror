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

"""Corpus scoreboard of the item-level dump (package STAB-PCB), used by scoreboard.sh.

  scoreboard.py board REF RUST NAME OUT.json   classify one board (compare.py rules), print
                                               one line per section, write OUT.json
  scoreboard.py summary RESDIR [--classes N]   totals per section over every OUT.json, the
                                               boards with failing lines and the N most
                                               frequent failure classes (SECTION|FIELD)

The classification is compare.py's (match / mismatch / pending / rust-error / missing /
extra / ref-error); a failure class is SECTION|FIELD with digits folded to `N`.
"""

import gzip
import importlib.util
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("compare", os.path.join(HERE, "compare.py"))
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)

KINDS = compare.KINDS
FAILING = ["mismatch", "rust-error", "missing", "extra"]


def open_any(path):
    if path.endswith(".gz"):
        return gzip.open(path, "rt", encoding="utf-8", errors="surrogateescape")

    return open(path, encoding="utf-8", errors="surrogateescape")


def parse(path):
    # compare.parse opens the file itself; feed it a decompressed copy when needed.
    if not path.endswith(".gz"):
        return compare.parse(path)

    tmp = path[:-3] + ".plain"

    with open_any(path) as src, open(tmp, "w", encoding="utf-8", errors="surrogateescape") as dst:
        for line in src:
            dst.write(line)

    try:
        return compare.parse(tmp)
    finally:
        os.remove(tmp)


def classify(ref_path, rust_path):
    """The result list of compare.main: (kind, key, ref value, rust value)."""
    import fnmatch

    ref, _, _, ref_errors = parse(ref_path)
    rust, pending, rust_errors, _ = parse(rust_path)
    patterns = {}

    for source, kind in ((pending, "pending"), (rust_errors, "rust-error")):
        for k, v in source.items():
            if "*" in k.split("|", 3)[3]:
                head, field = k.rsplit("|", 1)
                patterns.setdefault(head, []).append((field, kind, v))

    def pattern_failure(k):
        head, field = k.rsplit("|", 1)

        for pat, kind, v in patterns.get(head, []):
            if fnmatch.fnmatchcase(field, pat):
                return kind, v

        return None

    results = []

    for k, v in ref.items():
        if k in rust:
            results.append(("match" if rust[k] == v else "mismatch", k, v, rust[k]))
        elif k in pending:
            results.append(("pending", k, v, pending[k]))
        elif k in rust_errors:
            results.append(("rust-error", k, v, rust_errors[k]))
        elif pattern_failure(k) is not None:
            kind, pv = pattern_failure(k)
            results.append((kind, k, v, pv))
        else:
            results.append(("missing", k, v, None))

    for source in (rust, pending, rust_errors):
        for k, v in source.items():
            if k in ref or "*" in k.split("|", 3)[3]:
                continue

            if compare.covered_by_ref_error(k, ref_errors):
                results.append(("ref-error", k, None, v))
            elif source is rust:
                results.append(("extra", k, None, v))

    return results, len(ref)


def field_class(full_key):
    _, section, _, field = full_key.split("|", 3)
    field = re.sub(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", "U", field)
    return section + "|" + re.sub(r"\d+", "N", field)


def cmd_board(ref_path, rust_path, name, out_path):
    results, ref_count = classify(ref_path, rust_path)
    sections, classes = {}, {}

    for kind, k, rv, sv in results:
        sec = k.split("|", 2)[1]
        sections.setdefault(sec, {x: 0 for x in KINDS})[kind] += 1

        if kind in FAILING or kind == "pending":
            c = classes.setdefault(kind + " " + field_class(k), {"n": 0, "example": None})
            c["n"] += 1

            if c["example"] is None:
                c["example"] = [k.split("|", 3)[2], rv, sv]

    data = {"board": name, "ref_lines": ref_count, "sections": sections, "classes": classes}

    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=0, sort_keys=True)

    if ref_count == 0:
        print(f"{name} - ref-empty")

    for sec in sorted(sections):
        s = sections[sec]
        print(f"{name} {sec} " + " ".join(f"{x}={s[x]}" for x in KINDS if s[x]))


def cmd_summary(res_dir, n_classes):
    totals, boards, classes, empty, n_boards = {}, [], {}, [], 0

    for fn in sorted(os.listdir(res_dir)):
        if not fn.endswith(".json"):
            continue

        with open(os.path.join(res_dir, fn), encoding="utf-8") as f:
            d = json.load(f)

        n_boards += 1

        if d["ref_lines"] == 0:
            empty.append(d["board"])

        bad = {}

        for sec, s in d["sections"].items():
            t = totals.setdefault(sec, {x: 0 for x in KINDS})

            for x in KINDS:
                t[x] += s[x]

                if x in FAILING and s[x]:
                    bad[sec + "." + x] = bad.get(sec + "." + x, 0) + s[x]

        if bad:
            boards.append((d["board"], bad))

        for c, v in d["classes"].items():
            e = classes.setdefault(c, {"n": 0, "boards": 0, "first": d["board"], "example": v["example"]})
            e["n"] += v["n"]
            e["boards"] += 1

    print("# pcb-dump --no-init vs pcb-dump-rs over the corpus (tools/pcb-dump/scoreboard.sh)")
    print(f"# boards: {n_boards}, reference without output: {len(empty)}")
    w = max([len(s) for s in totals] + [5])
    print(f"{'section':<{w}} " + " ".join(f"{x:>10}" for x in KINDS))

    for sec in sorted(totals):
        print(f"{sec:<{w}} " + " ".join(f"{totals[sec][x]:>10}" for x in KINDS))

    grand = {x: sum(t[x] for t in totals.values()) for x in KINDS}
    print(f"{'TOTAL':<{w}} " + " ".join(f"{grand[x]:>10}" for x in KINDS))
    print()
    print("# boards with failing lines (section.kind=count)")

    for b, bad in boards:
        print(b + " " + " ".join(f"{k}={v}" for k, v in sorted(bad.items())))

    for b in empty:
        print(b + " ref-empty")

    print()
    print(f"# failure / pending classes (kind section|field: values, boards, first board)")

    for c, e in sorted(classes.items(), key=lambda kv: (-kv[1]["n"], kv[0]))[:n_classes]:
        print(f"{c}: {e['n']} values, {e['boards']} boards, {e['first']}")


def main():
    if len(sys.argv) >= 6 and sys.argv[1] == "board":
        cmd_board(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5])
    elif len(sys.argv) >= 3 and sys.argv[1] == "summary":
        n = 1000

        if len(sys.argv) >= 5 and sys.argv[3] == "--classes":
            n = int(sys.argv[4])

        cmd_summary(sys.argv[2], n)
    else:
        print(__doc__)
        sys.exit(2)


if __name__ == "__main__":
    main()
