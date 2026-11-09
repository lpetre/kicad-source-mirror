#!/usr/bin/env python3
"""Targeted cases for the deliberate upstream deviations listed in the clipper2-rs README.
Usage: gen_quirks.py <out>"""
import random, struct, sys
rng = random.Random(99)
out = []
cid = 0
def bits(f): return struct.unpack("<Q", struct.pack("<d", float(f)))[0]
def emit(cat, args, groups):
    global cid
    cid += 1
    z = 1
    lines = ["CASE %d %s OFFSET %s" % (cid, cat, " ".join(str(a) for a in args))]
    for g in groups:
        lines.append("G")
        for p in g:
            parts = ["S", str(len(p))]
            for (x, y) in p:
                parts.append("%d %d %d" % (x, y, z)); z += 1
            lines.append(" ".join(parts))
    lines.append("END")
    out.append("\n".join(lines))
def rect(x0, y0, x1, y1): return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
def rpoly(n, lo, hi): return [(rng.randint(lo, hi), rng.randint(lo, hi)) for _ in range(n)]
degenerate_groups = [
    [[(0, 0), (1000, 0)]],                                 # 2-point "polygon"
    [[(0, 0), (500, 0), (1000, 0)]],                       # collinear
    [[(0, 0), (1000, 1000), (0, 0), (1000, 1000)]],        # zero area back-and-forth
    [[(5, 5)]],                                            # single point
    [[(0, 0), (100, 0), (100, 100), (0, 100), (0, 0), (0, 100), (100, 100), (100, 0)]],  # area cancels
    [[]],                                                  # empty path
    [[], [(0, 0), (1000, 0)]],
]
for dg in degenerate_groups:
    for jt in (0, 1, 2, 3):
        for delta in (-50.0, 50.0, -7.0, 7.0, -300.0):
            for _ in range(3):
                good1 = [rect(2000, 2000, 4000, 3500)]
                good2 = [rpoly(rng.randint(3, 8), 5000, 9000)]
                good3 = [list(reversed(rect(-4000, -4000, -1000, -2000)))]
                # degenerate first, in the middle, last
                emit("quirk_degenerate_group_first", (jt, 0, bits(2.0), bits(0.0), bits(delta)), [dg, good1, good2, good3])
                emit("quirk_degenerate_group_middle", (jt, 0, bits(2.0), bits(0.0), bits(delta)), [good1, dg, good2, good3])
                emit("quirk_degenerate_group_last", (jt, 0, bits(2.0), bits(0.0), bits(delta)), [good1, good2, good3, dg])
open(sys.argv[1], "w").write("\n".join(out) + "\n")
print("cases", cid)
