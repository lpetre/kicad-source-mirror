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

"""SET-5: compare the project-dependent fields of the `board` section of a C++ `pcb-dump board`
output (REF) with a `pcb-dump-rs board` output (RUST) of the same board and load path.

Compared: has_project, every ds.* field (ds.meander.*: no Rust API), title_block.*,
composite_netclass.*, and per net *name* (not code: net codes are assigned while parsing and
differ as long as items are pending in the Rust parser) the stored netclass (net.N.netclass)
and the effective netclass (effective_netclass.N). A REF net absent from RUST is `pending`
(its items are not loaded yet). Prints one line `STATUS name counts` and the problems; exit 0.
"""

import sys

SCALAR_PREFIXES = ("ds.", "title_block.", "composite_netclass.", "has_project")
SKIP_PREFIXES = ("ds.meander.",)
# Fields that need a session step (DRC engine: QueryWorstConstraint), not run by pcb-dump-rs.
SESSION_FIELDS = {"ds.biggest_clearance"}


def parse(path):
    values, pending, load_error = {}, set(), False

    with open(path, encoding="utf-8", errors="surrogateescape") as f:
        for raw in f:
            line = raw.rstrip("\n")
            kind = line.split("|", 1)[0]

            if kind == "error" and line.startswith("error|load|"):
                load_error = True
                continue

            if kind == "pending":
                body = line[len("pending|"):]
                if body.startswith("board|board|"):
                    pending.add(body[len("board|board|"):].split("=", 1)[0])
                continue

            if kind != "board" or not line.startswith("board|board|"):
                continue

            field, _, value = line[len("board|board|"):].partition("=")
            values[field] = value

    return values, pending, load_error


def nets(values):
    """name -> (netclass, effective netclass) keyed by net name."""
    by_code = {}

    for field, value in values.items():
        parts = field.split(".")

        if parts[0] == "net" and len(parts) == 3:
            by_code.setdefault(parts[1], {})[parts[2]] = value
        elif parts[0] == "effective_netclass" and len(parts) == 2:
            by_code.setdefault(parts[1], {})["effective"] = value

    return {d.get("name"): (d.get("netclass"), d.get("effective")) for d in by_code.values()
            if "name" in d}


def main():
    ref_path, rust_path, name = sys.argv[1], sys.argv[2], sys.argv[3]
    ref, _, ref_err = parse(ref_path)
    rust, rust_pending, rust_err = parse(rust_path)

    if not rust and not rust_err:
        rust_err = True  # the Rust parser panicked (a construct without allow_pending support)

    if ref_err or rust_err:
        print(f"LOADERR {name} ref_error={ref_err} rust_error={rust_err}")
        return

    counts = {"match": 0, "mismatch": 0, "pending": 0, "session": 0, "missing": 0}
    problems = []

    for field, value in ref.items():
        if not field.startswith(SCALAR_PREFIXES) or field.startswith(SKIP_PREFIXES):
            continue

        if field in SESSION_FIELDS:
            counts["session"] += 1
        elif field in rust_pending:
            counts["pending"] += 1
        elif field not in rust:
            counts["missing"] += 1
            problems.append(f"  missing {field} ref={value}")
        elif rust[field] == value:
            counts["match"] += 1
        else:
            counts["mismatch"] += 1
            problems.append(f"  mismatch {field} ref={value} rust={rust[field]}")

    for field in rust:
        if field.startswith(SCALAR_PREFIXES) and field not in ref:
            counts["mismatch"] += 1
            problems.append(f"  extra {field} rust={rust[field]}")

    ref_nets, rust_nets = nets(ref), nets(rust)

    for net, (nc, eff) in ref_nets.items():
        if net not in rust_nets:
            counts["pending"] += 2
            continue

        rnc, reff = rust_nets[net]

        for what, a, b in (("netclass", nc, rnc), ("effective", eff, reff)):
            if a == b:
                counts["match"] += 1
            else:
                counts["mismatch"] += 1
                problems.append(f"  mismatch net {net} {what} ref={a} rust={b}")

    status = "MATCH" if counts["mismatch"] == 0 and counts["missing"] == 0 else "MISMATCH"
    print(f"{status} {name} " + " ".join(f"{k}={v}" for k, v in counts.items()))

    for p in problems[:40]:
        print(p)


if __name__ == "__main__":
    main()
