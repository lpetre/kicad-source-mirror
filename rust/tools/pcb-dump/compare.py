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

"""Compare a C++ pcb-dump output (REF) with a pcb-dump-rs output (RUST).

Both are parsed into maps keyed by SECTION|KEY|FIELD (one map per `file|` block, the file name
is part of the key when there are several). Each REF key is classified as

  match       same value
  mismatch    different value
  pending     RUST has `pending|SECTION|KEY|FIELD=CXX::Name` (or a `pending|SECTION|KEY|*`
              line covering the whole key)
  rust-error  RUST has `rust-error|SECTION|KEY|FIELD=message`
  missing     no RUST line at all

and RUST data lines without a REF line are `extra`. REF `error|` lines ("no value", e.g. a wx
assertion) cover the REF-less RUST lines of that field: those are counted as `ref-error` and
never fail. Exit code 0 only when there are no mismatch / rust-error / missing / extra lines.
"""

import argparse
import json
import sys
from collections import OrderedDict

KINDS = ["match", "mismatch", "pending", "rust-error", "missing", "extra", "ref-error"]
FAILING = {"mismatch", "rust-error", "missing", "extra"}


def parse(path):
    """Returns (values, pending, rust_errors, ref_errors) keyed by 'FILE|SECTION|KEY|FIELD'."""
    values, pending, errors, ref_errors = OrderedDict(), {}, {}, {}
    file_name = ""

    with open(path, encoding="utf-8", errors="surrogateescape") as f:
        for raw in f:
            line = raw.rstrip("\n")

            if not line:
                continue

            kind = line.split("|", 1)[0]

            if kind == "file":
                file_name = line.split("|")[1]
                continue

            if kind == "warn":
                continue

            if kind in ("pending", "rust-error"):
                _, section, key, rest = line.split("|", 3)
                field, _, value = rest.partition("=")
                target = pending if kind == "pending" else errors
                target[f"{file_name}|{section}|{key}|{field}"] = value
                continue

            if kind == "error":
                parts = line.split("|", 3)

                if len(parts) == 4:
                    _, section, key, msg = parts
                    what = msg.split(":", 1)[0]
                    ref_errors.setdefault(f"{file_name}|{section}|{key}", []).append(what)

                continue

            section, key, rest = line.split("|", 2)
            field, _, value = rest.partition("=")
            values[f"{file_name}|{section}|{key}|{field}"] = value

    return values, pending, errors, ref_errors


def covered_by_ref_error(full_key, ref_errors):
    """A REF `error|S|K|what: ...` line covers field F of S|K when `what` is a dot-aligned part
    of F (C++ guards report the bare field name, a group name such as `tpoly`, or the section
    name for a whole section)."""
    file_name, section, key, field = full_key.split("|", 3)
    whats = ref_errors.get(f"{file_name}|{section}|{key}", [])
    parts = field.split(".")

    for w in whats:
        wp = w.split(".")

        for i in range(len(parts) - len(wp) + 1):
            if parts[i:i + len(wp)] == wp:
                return True

    return False


def group_of(full_key, by_class):
    _, section, key, _ = full_key.split("|", 3)
    return f"{section}|{key}" if by_class else section


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ref")
    ap.add_argument("rust")
    ap.add_argument("--section", help="only this section (e.g. pad, defaults)")
    ap.add_argument("--show", type=int, default=20, help="print the first N problems of each kind")
    ap.add_argument("--summary-json", action="store_true", help="print the scoreboard as JSON")
    ap.add_argument("--by-class", action="store_true",
                    help="group by SECTION|KEY (default for the defaults section)")
    args = ap.parse_args()

    ref, _, _, ref_errors = parse(args.ref)
    rust, pending, rust_errors, _ = parse(args.rust)

    def wanted(k):
        return args.section is None or k.split("|", 2)[1] == args.section

    def key_pending(k):
        file_name, section, key, _ = k.split("|", 3)
        return pending.get(f"{file_name}|{section}|{key}|*")

    results = []   # (kind, key, ref value, rust value)

    for k, v in ref.items():
        if not wanted(k):
            continue

        if k in rust:
            results.append(("match" if rust[k] == v else "mismatch", k, v, rust[k]))
        elif k in pending:
            results.append(("pending", k, v, pending[k]))
        elif key_pending(k) is not None:
            results.append(("pending", k, v, key_pending(k)))
        elif k in rust_errors:
            results.append(("rust-error", k, v, rust_errors[k]))
        else:
            results.append(("missing", k, v, None))

    for source in (rust, pending, rust_errors):
        for k, v in source.items():
            if not wanted(k) or k in ref or k.endswith("|*"):
                continue

            if covered_by_ref_error(k, ref_errors):
                results.append(("ref-error", k, None, v))
            elif source is rust:
                results.append(("extra", k, None, v))

    by_class = args.by_class or args.section == "defaults" or all(
        k.split("|", 2)[1] == "defaults" for _, k, _, _ in results)
    board = OrderedDict()

    for kind, k, _, _ in results:
        g = group_of(k, by_class)
        board.setdefault(g, {x: 0 for x in KINDS})[kind] += 1

    total = {x: sum(b[x] for b in board.values()) for x in KINDS}

    if args.summary_json:
        print(json.dumps({"groups": board, "total": total}, indent=1))
    else:
        w = max([len(g) for g in board] + [5])
        print(f"{'group':<{w}} " + " ".join(f"{x:>10}" for x in KINDS))

        for g, b in board.items():
            print(f"{g:<{w}} " + " ".join(f"{b[x]:>10}" for x in KINDS))

        print(f"{'TOTAL':<{w}} " + " ".join(f"{total[x]:>10}" for x in KINDS))

        for kind in ["mismatch", "rust-error", "missing", "extra"]:
            shown = [r for r in results if r[0] == kind][: args.show]

            if shown:
                print(f"\n-- first {len(shown)} {kind}:")

            for _, k, rv, sv in shown:
                print(f"{k}\n    ref:  {rv}\n    rust: {sv}")

    return 0 if all(total[x] == 0 for x in FAILING) else 1


if __name__ == "__main__":
    sys.exit(main())
