#!/usr/bin/env python3
"""Compare two driver outputs case by case.

Usage: compare.py <cases.txt> <ref_out> <cand_out> [--show N] [--list FILE]

Match levels per case (best first):
  exact      every output line identical (coordinates, z, vertex/contour order, tree shape,
             z-callback invocation sequence)
  result     all result sections (TREE/PATHS/TREE2/A/OK) identical incl. z; only the z-callback
             log (argument values / call order) differs
  xy         result sections identical when z is ignored (same vertices, same order)
  canon      same contours as point cycles (any start vertex), any contour order / nesting
  DIFF       geometry differs
"""
import sys


def load(path):
    cases = {}
    cur = None
    with open(path, "r") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith("CASE "):
                cur = []
                cases[int(line.split()[1])] = cur
            elif line == "END":
                cur = None
            elif cur is not None:
                cur.append(line)
    return cases


def load_meta(path):
    meta = {}
    with open(path, "r") as f:
        for line in f:
            if line.startswith("CASE "):
                h = line.split()
                meta[int(h[1])] = (h[2], h[3], h[4:])
    return meta


def results_only(lines):
    return [l for l in lines if not (l.startswith("Z ") or l.startswith("Y "))]


def strip_z(line):
    t = line.split()
    if t[0] == "N":
        head, nums = t[:4], t[4:]
    elif t[0] == "Q":
        head, nums = t[:2], t[2:]
    else:
        return line
    xy = [nums[i] + "," + nums[i + 1] for i in range(0, len(nums) - 2, 3)]
    return " ".join(head + xy)


def canon_sections(lines):
    """section title -> sorted list of canonical cycles."""
    secs = {}
    cur = None
    for l in lines:
        t = l.split()
        if t[0] in ("TREE", "PATHS", "TREE2"):
            cur = secs.setdefault(t[0], [])
        elif t[0] in ("N", "Q") and cur is not None:
            nums = t[4:] if t[0] == "N" else t[2:]
            pts = [(int(nums[i]), int(nums[i + 1])) for i in range(0, len(nums) - 2, 3)]
            if pts:
                k = pts.index(min(pts))
                pts = pts[k:] + pts[:k]
            cur.append(tuple(pts))
        elif t[0] == "A":
            secs.setdefault("A", []).append(l)
        elif t[0] in ("ERR", "PANIC"):
            secs.setdefault("ERR", []).append(t[0])
    return {k: sorted(v) for k, v in secs.items()}


def classify(a, b):
    if a == b:
        return "exact"
    ra, rb = results_only(a), results_only(b)
    if ra == rb:
        return "result"
    if [strip_z(l) for l in ra] == [strip_z(l) for l in rb]:
        return "xy"
    if canon_sections(ra) == canon_sections(rb):
        return "canon"
    return "DIFF"


def main():
    meta = load_meta(sys.argv[1])
    ref = load(sys.argv[2])
    cand = load(sys.argv[3])
    show = 0
    listfile = None
    if "--show" in sys.argv:
        show = int(sys.argv[sys.argv.index("--show") + 1])
    if "--list" in sys.argv:
        listfile = open(sys.argv[sys.argv.index("--list") + 1], "w")
    levels = ("exact", "result", "xy", "canon", "DIFF", "missing")
    table = {}
    total = dict.fromkeys(levels, 0)
    shown = 0
    for cid in sorted(ref):
        cat, op, args = meta[cid]
        key = cat.split("/")[0] + " " + op
        row = table.setdefault(key, dict.fromkeys(levels, 0))
        if cid not in cand:
            lv = "missing"
        else:
            lv = classify(ref[cid], cand[cid])
        row[lv] += 1
        total[lv] += 1
        if lv != "exact" and listfile:
            listfile.write("%d %s %s %s %s\n" % (cid, lv, cat, op, " ".join(args)))
        if lv not in ("exact",) and shown < show:
            shown += 1
            print("---- case %d %s %s %s : %s" % (cid, cat, op, " ".join(args), lv))
            ra, rb = results_only(ref[cid]), results_only(cand[cid])
            n = 0
            for i in range(max(len(ra), len(rb))):
                la = ra[i] if i < len(ra) else "<none>"
                lb = rb[i] if i < len(rb) else "<none>"
                if la != lb:
                    print("  ref : " + la[:300])
                    print("  cand: " + lb[:300])
                    n += 1
                    if n >= 4:
                        break
    print("%-34s %6s %6s %6s %6s %6s %6s %6s" % (("category/op", "n") + levels[:5] + ("exact%",)))
    for key in sorted(table):
        row = table[key]
        n = sum(row.values())
        print("%-34s %6d %6d %6d %6d %6d %6d %6.2f" % (key, n, row["exact"], row["result"], row["xy"],
                                                       row["canon"], row["DIFF"] + row["missing"],
                                                       100.0 * row["exact"] / n))
    n = sum(total.values())
    print("%-34s %6d %6d %6d %6d %6d %6d %6.2f" % ("TOTAL", n, total["exact"], total["result"], total["xy"],
                                                   total["canon"], total["DIFF"] + total["missing"],
                                                   100.0 * total["exact"] / n))


main()
