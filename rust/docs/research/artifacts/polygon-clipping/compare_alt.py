#!/usr/bin/env python3
"""Compare the TREE section (xy only) of a non-Clipper candidate with the Clipper2 reference.
Usage: compare_alt.py <cases> <ref_out> <cand_out> [fr_filter]
Levels: exact (same contours, same order, same start vertex), canon (same contours as point
cycles, any order/start), area (same total signed area only), DIFF."""
import sys

def load(path):
    cases = {}; cur = None; intree = False
    for line in open(path):
        t = line.split()
        if not t: continue
        if t[0] == "CASE":
            cur = []; cases[int(t[1])] = cur; intree = False
        elif t[0] == "END": cur = None
        elif cur is None: continue
        elif t[0] == "TREE": intree = True
        elif t[0] in ("PATHS", "TREE2", "OK"): intree = False if t[0] != "OK" else intree
        elif t[0] == "PANIC": cur.append("PANIC")
        elif t[0] == "N" and intree:
            nums = t[4:]
            cur.append(tuple((int(float(nums[i])), int(float(nums[i+1]))) for i in range(0, len(nums)-2, 3)))
    return cases

def meta(path):
    m = {}
    for line in open(path):
        if line.startswith("CASE "):
            h = line.split(); m[int(h[1])] = (h[2].split("/")[0], h[3], h[4:])
    return m

def area2(p):
    return sum(p[i][0]*p[(i+1)%len(p)][1] - p[(i+1)%len(p)][0]*p[i][1] for i in range(len(p)))

def canon(p):
    k = p.index(min(p)); return p[k:] + p[:k]

m = meta(sys.argv[1]); ref = load(sys.argv[2]); cand = load(sys.argv[3])
frf = sys.argv[4] if len(sys.argv) > 4 else None
tab = {}
for cid, c in cand.items():
    cat, op, args = m[cid]
    if frf is not None and args[1] != frf: continue
    r = ref[cid]
    row = tab.setdefault(cat, dict(n=0, exact=0, canon=0, area=0, DIFF=0, panic=0))
    row["n"] += 1
    if "PANIC" in c: row["panic"] += 1; continue
    if r == c: row["exact"] += 1
    elif sorted(canon(p) for p in r if p) == sorted(canon(p) for p in c if p): row["canon"] += 1
    elif sum(area2(p) for p in r) == sum(area2(p) for p in c): row["area"] += 1
    else: row["DIFF"] += 1
tot = dict(n=0, exact=0, canon=0, area=0, DIFF=0, panic=0)
print("%-26s %6s %6s %6s %6s %6s %6s" % ("category", "n", "exact", "canon", "area", "DIFF", "panic"))
for k in sorted(tab):
    r = tab[k]
    for x in tot: tot[x] += r[x]
    print("%-26s %6d %6d %6d %6d %6d %6d" % (k, r["n"], r["exact"], r["canon"], r["area"], r["DIFF"], r["panic"]))
print("%-26s %6d %6d %6d %6d %6d %6d" % ("TOTAL", tot["n"], tot["exact"], tot["canon"], tot["area"], tot["DIFF"], tot["panic"]))
