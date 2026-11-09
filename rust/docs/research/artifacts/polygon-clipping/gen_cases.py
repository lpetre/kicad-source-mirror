#!/usr/bin/env python3
"""Deterministic test-case generator for the Clipper2 differential experiment.

Usage: gen_cases.py <out_file> [<upstream Polygons.txt> [<seed>]]

Case file format (one case):
    CASE <id> <category> <OP> <args...>
    G                       (optional: starts a new offset group)
    S <n> x y z x y z ...   (subject path, z always present)
    C <n> x y z ...         (clip path)
    END
"""
import math
import random
import struct
import sys

SEED = int(sys.argv[3]) if len(sys.argv) > 3 else 20261109
rng = random.Random(SEED)
out = []
case_id = 0

CT_INTERSECTION, CT_UNION, CT_DIFFERENCE, CT_XOR = 1, 2, 3, 4
FR_EVENODD, FR_NONZERO, FR_POSITIVE, FR_NEGATIVE = 0, 1, 2, 3
JT_SQUARE, JT_BEVEL, JT_ROUND, JT_MITER = 0, 1, 2, 3
ET_POLYGON, ET_JOINED, ET_BUTT, ET_SQUARE, ET_ROUND = 0, 1, 2, 3, 4


def bits(f):
    return struct.unpack("<Q", struct.pack("<d", float(f)))[0]


def area2(p):
    a = 0
    n = len(p)
    for i in range(n):
        x0, y0 = p[i]
        x1, y1 = p[(i + 1) % n]
        a += x0 * y1 - x1 * y0
    return a


def oriented(p, positive=True):
    a = area2(p)
    if (a >= 0) != positive:
        return list(reversed(p))
    return list(p)


def emit(cat, op, args, subj=(), clip=(), groups=None):
    """subj/clip: lists of paths (list of (x, y)); groups: list of lists of paths (offset)."""
    global case_id
    case_id += 1
    z = 0
    lines = ["CASE %d %s %s %s" % (case_id, cat, op, " ".join(str(a) for a in args))]

    def path_line(tag, p):
        nonlocal z
        parts = [tag, str(len(p))]
        for (x, y) in p:
            parts.append("%d %d %d" % (x, y, z))
            z += 1
        lines.append(" ".join(parts))

    if groups is not None:
        for g in groups:
            lines.append("G")
            for p in g:
                path_line("S", p)
    else:
        for p in subj:
            path_line("S", p)
    for p in clip:
        path_line("C", p)
    lines.append("END")
    out.append("\n".join(lines))


# ---------------------------------------------------------------- shapes

def rnd_poly(n, lo, hi):
    return [(rng.randint(lo, hi), rng.randint(lo, hi)) for _ in range(n)]


def rect(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def circle(cx, cy, r, n, phase=0.0):
    pts = []
    for i in range(n):
        a = phase + 2 * math.pi * i / n
        pts.append((cx + int(round(r * math.cos(a))), cy + int(round(r * math.sin(a)))))
    return pts


def oval(x0, y0, x1, y1, w, n):
    """Track-like stadium between two points, n segments per half circle."""
    ang = math.atan2(y1 - y0, x1 - x0)
    r = w / 2
    pts = []
    for i in range(n + 1):
        a = ang - math.pi / 2 + math.pi * i / n
        pts.append((x1 + int(round(r * math.cos(a))), y1 + int(round(r * math.sin(a)))))
    for i in range(n + 1):
        a = ang + math.pi / 2 + math.pi * i / n
        pts.append((x0 + int(round(r * math.cos(a))), y0 + int(round(r * math.sin(a)))))
    return pts


def roundrect(x0, y0, x1, y1, r, n):
    pts = []
    corners = [(x1 - r, y1 - r, 0), (x0 + r, y1 - r, 1), (x0 + r, y0 + r, 2), (x1 - r, y0 + r, 3)]
    for (cx, cy, q) in corners:
        for i in range(n + 1):
            a = q * math.pi / 2 + (math.pi / 2) * i / n
            pts.append((cx + int(round(r * math.cos(a))), cy + int(round(r * math.sin(a)))))
    return pts


def kicad_seg_count(radius, err_max, min_seg=8):
    """Port of GetArcToSegmentCount( aRadius, aErrorMax, FULL_CIRCLE )."""
    radius = max(1, radius)
    err_max = max(1, err_max)
    rel_error = err_max / radius
    arc_increment = 180 / math.pi * math.acos(1.0 - rel_error) * 2 if rel_error < 2 else 360.0
    arc_increment = min(360.0 / min_seg, arc_increment)
    v = 360.0 / arc_increment
    seg = int(math.floor(v + 0.5))
    return max(seg, 2)


def star(cx, cy, r, n, w):
    """n thin triangles (blades) all sharing the centre: many edges through one point."""
    paths = []
    for i in range(n):
        a = 2 * math.pi * i / n + rng.random() * 0.01
        x = cx + int(round(r * math.cos(a)))
        y = cy + int(round(r * math.sin(a)))
        xb = cx - int(round(r * math.cos(a)))
        yb = cy - int(round(r * math.sin(a)))
        nx = int(round(w * -math.sin(a)))
        ny = int(round(w * math.cos(a)))
        paths.append([(xb, yb), (x + nx, y + ny), (x - nx, y - ny)])
    return paths


ALL_CT = (CT_INTERSECTION, CT_UNION, CT_DIFFERENCE, CT_XOR)
MAIN_FR = (FR_NONZERO, FR_EVENODD)


def bool_cases(cat, subj, clip, cts=ALL_CT, frs=MAIN_FR, pc=1):
    for ct in cts:
        for fr in frs:
            emit(cat, "BOOL", (ct, fr, pc), subj, clip)


# ---------------------------------------------------------------- boolean categories

def gen_bool():
    # a. small grid: maximally degenerate random polygons
    for _ in range(160):
        g = rng.choice((4, 6, 8, 10, 16))
        subj = [rnd_poly(rng.randint(3, 9), 0, g) for _ in range(rng.randint(1, 3))]
        clip = [rnd_poly(rng.randint(3, 9), 0, g) for _ in range(rng.randint(1, 3))]
        bool_cases("rnd_small", subj, clip)
    for _ in range(60):
        g = rng.choice((5, 8, 12))
        subj = [rnd_poly(rng.randint(3, 8), 0, g) for _ in range(rng.randint(1, 3))]
        clip = [rnd_poly(rng.randint(3, 8), 0, g) for _ in range(rng.randint(1, 2))]
        bool_cases("rnd_small_posneg", subj, clip, frs=(FR_POSITIVE, FR_NEGATIVE))
    for _ in range(60):
        g = rng.choice((5, 8, 12))
        subj = [rnd_poly(rng.randint(3, 8), 0, g) for _ in range(rng.randint(1, 3))]
        clip = [rnd_poly(rng.randint(3, 8), 0, g) for _ in range(rng.randint(1, 2))]
        bool_cases("rnd_small_nocollinear", subj, clip, pc=0)
    # b. mid range
    for _ in range(60):
        subj = [rnd_poly(rng.randint(3, 30), 0, 1000) for _ in range(rng.randint(1, 3))]
        clip = [rnd_poly(rng.randint(3, 30), 0, 1000) for _ in range(rng.randint(1, 3))]
        bool_cases("rnd_mid", subj, clip)
    # larger self-intersecting polygons (many intersections per scanbeam)
    for _ in range(25):
        subj = [rnd_poly(rng.randint(40, 120), 0, 400)]
        clip = [rnd_poly(rng.randint(40, 120), 0, 400)]
        bool_cases("rnd_dense", subj, clip)
    # c. KiCad coordinate range (int32 nanometres)
    for _ in range(40):
        lim = 2_000_000_000
        subj = [rnd_poly(rng.randint(3, 20), -lim, lim) for _ in range(rng.randint(1, 3))]
        clip = [rnd_poly(rng.randint(3, 20), -lim, lim) for _ in range(rng.randint(1, 3))]
        bool_cases("rnd_i32", subj, clip)
    # beyond KiCad range: exercises the 128-bit code paths
    for _ in range(12):
        lim = 10 ** 18
        subj = [rnd_poly(rng.randint(3, 12), -lim, lim) for _ in range(rng.randint(1, 2))]
        clip = [rnd_poly(rng.randint(3, 12), -lim, lim) for _ in range(rng.randint(1, 2))]
        bool_cases("rnd_i62", subj, clip)
    # d. grid aligned rectangles (shared edges / corners everywhere)
    for _ in range(60):
        def rr():
            x0 = rng.randint(0, 8) * 10
            y0 = rng.randint(0, 8) * 10
            return rect(x0, y0, x0 + rng.randint(1, 4) * 10, y0 + rng.randint(1, 4) * 10)
        subj = [rr() for _ in range(rng.randint(1, 5))]
        clip = [rr() for _ in range(rng.randint(1, 5))]
        if rng.random() < 0.3:
            subj = [oriented(p, rng.random() < 0.5) for p in subj]
        bool_cases("rect_grid", subj, clip)
    for _ in range(30):
        def rr():
            x0 = rng.randint(0, 8) * 10
            y0 = rng.randint(0, 8) * 10
            return rect(x0, y0, x0 + rng.randint(1, 4) * 10, y0 + rng.randint(1, 4) * 10)
        subj = [rr() for _ in range(rng.randint(1, 5))]
        clip = [rr() for _ in range(rng.randint(1, 5))]
        bool_cases("rect_grid_nocollinear", subj, clip, pc=0)
    # e. KiCad-like: zone outline minus clearance holes of pads / vias / tracks (nm)
    for _ in range(60):
        mm = 1_000_000
        w = rng.randint(10, 60) * mm
        h = rng.randint(10, 60) * mm
        zone = [rect(0, 0, w, h)]
        if rng.random() < 0.4:
            zone = [oriented(rnd_poly(rng.randint(4, 10), 0, max(w, h)), True)]
        holes = []
        for _ in range(rng.randint(3, 40)):
            kind = rng.random()
            cx = rng.randint(-2 * mm, w + 2 * mm)
            cy = rng.randint(-2 * mm, h + 2 * mm)
            if kind < 0.4:
                r = rng.randint(150_000, 2_500_000)
                n = kicad_seg_count(r, 5000)
                holes.append(circle(cx, cy, r, n, math.pi / n))
            elif kind < 0.75:
                ln = rng.randint(mm // 2, 15 * mm)
                ang = rng.choice((0, 45, 90, 135, 180, 225, 270, 315, rng.uniform(0, 360)))
                x1 = cx + int(round(ln * math.cos(math.radians(ang))))
                y1 = cy + int(round(ln * math.sin(math.radians(ang))))
                wd = rng.choice((150_000, 200_000, 250_000, 400_000, 1_000_000)) + 2 * 200_000
                holes.append(oval(cx, cy, x1, y1, wd, max(4, kicad_seg_count(wd // 2, 5000) // 2)))
            elif kind < 0.9:
                a = rng.randint(mm // 2, 3 * mm)
                b = rng.randint(mm // 2, 3 * mm)
                holes.append(rect(cx - a, cy - b, cx + a, cy + b))
            else:
                a = rng.randint(mm, 3 * mm)
                b = rng.randint(mm, 3 * mm)
                r = min(a, b) // rng.choice((2, 3, 4))
                holes.append(roundrect(cx - a, cy - b, cx + a, cy + b, r,
                                       max(2, kicad_seg_count(r, 5000) // 4)))
        emit("kicad_zone", "BOOL", (CT_DIFFERENCE, FR_NONZERO, 1), zone, holes)
        if rng.random() < 0.5:
            emit("kicad_zone", "BOOL", (CT_UNION, FR_NONZERO, 1), holes, [])
            emit("kicad_zone", "BOOL", (CT_INTERSECTION, FR_NONZERO, 1), zone, holes)
            emit("kicad_zone", "BOOL", (CT_XOR, FR_NONZERO, 1), zone, holes)
    # f. nested rings (holes, islands in holes)
    for _ in range(25):
        def rings(cx, cy, levels, n, r0):
            ps = []
            r = r0
            for lv in range(levels):
                c = circle(cx, cy, r, n, rng.random())
                ps.append(oriented(c, lv % 2 == 0))
                r = int(r * rng.uniform(0.5, 0.85))
                if r < 4:
                    break
            return ps
        subj = rings(rng.randint(-50, 50), rng.randint(-50, 50), rng.randint(1, 5),
                     rng.choice((4, 6, 12, 32)), rng.randint(200, 1000))
        clip = rings(rng.randint(-300, 300), rng.randint(-300, 300), rng.randint(1, 5),
                     rng.choice((4, 6, 12, 32)), rng.randint(200, 1000))
        bool_cases("nested_rings", subj, clip)
    # g. stars: many edges through one point in one scanbeam (std::sort tie order)
    for _ in range(40):
        n = rng.choice((6, 9, 12, 17, 24, 40))
        subj = star(0, 0, rng.choice((100, 1000, 100000)), n, rng.choice((3, 10, 50)))
        clip = star(rng.choice((0, 0, 1, 7)), rng.choice((0, 0, 3)), rng.choice((100, 1000, 100000)),
                    rng.choice((5, 8, 13, 21)), rng.choice((3, 10, 50)))
        bool_cases("star", subj, clip)
    # exact fans: integer lines through the origin (exactly coincident intersections)
    for _ in range(40):
        n = rng.choice((8, 12, 20, 32, 48))
        dirs = set()
        while len(dirs) < n:
            dx, dy = rng.randint(-12, 12), rng.randint(1, 12)
            dirs.add((dx, dy))
        k = rng.choice((10, 100, 1000))
        subj = []
        for (dx, dy) in sorted(dirs):
            w = rng.randint(1, 3)
            subj.append([(-dx * k, -dy * k), (dx * k + w, dy * k), (dx * k - w, dy * k)])
        clip = [rect(-5 * k, -5 * k, 5 * k, 5 * k)] if rng.random() < 0.5 else []
        bool_cases("fan_exact", subj, clip)
    # j. big rings (> 64 vertices) with many holes: PolyTree nesting / KiCad StripeIndex path
    for _ in range(30):
        n_outer = rng.choice((96, 128, 360, 720))
        r_outer = rng.randint(20_000_000, 50_000_000)
        subj = [circle(0, 0, r_outer, n_outer)]
        clip = []
        for _ in range(rng.randint(5, 60)):
            a = rng.uniform(0, 2 * math.pi)
            d = rng.uniform(0, r_outer * 1.05)
            r = rng.randint(200_000, 4_000_000)
            clip.append(circle(int(d * math.cos(a)), int(d * math.sin(a)), r,
                               rng.choice((8, 16, 32, 96, 200)), rng.random()))
        emit("big_rings", "BOOL", (CT_DIFFERENCE, FR_NONZERO, 1), subj, clip)
        emit("big_rings", "BOOL", (CT_XOR, FR_NONZERO, 1), subj, clip)
        # islands inside the holes as well
        isl = [circle(p[0][0] - 50_000, p[0][1], 40_000, 80) for p in clip[:10]]
        emit("big_rings", "BOOL", (CT_DIFFERENCE, FR_EVENODD, 1), subj + isl, clip)
    # comb shaped outlines (tall edges: StripeIndex declines) with holes
    for _ in range(15):
        teeth = rng.randint(40, 120)
        p = [(0, 0)]
        for i in range(teeth):
            p += [(i * 100 + 50, 0), (i * 100 + 50, 10000 + rng.randint(0, 50)),
                  (i * 100 + 80, 10000 + rng.randint(0, 50)), (i * 100 + 80, 0)]
        p += [(teeth * 100, 0), (teeth * 100, -5000), (0, -5000)]
        clip = [circle(rng.randint(100, teeth * 100 - 100), rng.randint(-4500, -500), rng.randint(5, 200),
                       rng.choice((8, 70))) for _ in range(rng.randint(3, 30))]
        emit("comb", "BOOL", (CT_DIFFERENCE, FR_NONZERO, 1), [p], clip)
        emit("comb", "BOOL", (CT_XOR, FR_EVENODD, 1), [p], clip)
    # k. KiCad fractured polygons (hole linked to outline by a zero width bridge): Simplify()
    for _ in range(60):
        mm = 1000
        w, h = rng.randint(20, 80) * mm, rng.randint(20, 80) * mm
        outline = rect(0, 0, w, h)
        path = []
        nholes = rng.randint(1, 6)
        xs = sorted(rng.sample(range(2 * mm, w - 2 * mm, 500), nholes))
        # walk bottom edge, dive up to each hole and back along the same edge
        path.append((0, 0))
        for x in xs:
            cy = rng.randint(3 * mm, h - 3 * mm)
            r = rng.randint(300, 1500)
            n = rng.choice((4, 8, 16))
            hole = [(x + int(round(r * math.cos(-math.pi / 2 - 2 * math.pi * i / n))),
                     cy + int(round(r * math.sin(-math.pi / 2 - 2 * math.pi * i / n)))) for i in range(n)]
            path.append((x, 0))
            path.extend(hole)
            path.append(hole[0])
            path.append((x, 0))
        path += [(w, 0), (w, h), (0, h)]
        bool_cases("fractured", [path], [], cts=(CT_UNION,), frs=(FR_NONZERO, FR_EVENODD))
        other = [rect(rng.randint(-5 * mm, w), rng.randint(-5 * mm, h),
                      rng.randint(0, w + 5 * mm), rng.randint(0, h + 5 * mm))]
        bool_cases("fractured", [path], other, frs=(FR_NONZERO,))


def gen_kicad_coincident():
    """PCB-like inputs with many exactly coincident vertices/edges: chained track ovals whose end
    caps are the same point set, duplicated pads, pads centred on track ends, thermal spokes,
    pad arrays on a grid, all subtracted from / united with a zone in a single call."""
    mm = 1_000_000

    def cap_circle(cx, cy, r, n):
        return circle(cx, cy, r, n, math.pi / n)

    for it in range(70):
        w, h = rng.randint(20, 80) * mm, rng.randint(20, 80) * mm
        zone = [rect(0, 0, w, h)]
        holes = []
        # routed nets: chains of 45 degree segments, each segment an oval; consecutive ovals
        # share the end point (and for collinear moves the whole cap)
        for _ in range(rng.randint(2, 12)):
            width = rng.choice((150_000, 200_000, 250_000, 500_000)) + 2 * rng.choice((150_000, 200_000))
            nseg = max(4, kicad_seg_count(width // 2, 5000) // 2)
            x, y = rng.randint(0, w // 100_000) * 100_000, rng.randint(0, h // 100_000) * 100_000
            d = rng.choice(((1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)))
            for _ in range(rng.randint(1, 8)):
                if rng.random() < 0.5:
                    d = rng.choice(((1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)))
                ln = rng.randint(1, 60) * 100_000
                x1, y1 = x + d[0] * ln, y + d[1] * ln
                holes.append(oval(x, y, x1, y1, width, nseg))
                if rng.random() < 0.3:
                    holes.append(oval(x, y, x1, y1, width, nseg))  # exact duplicate track
                if rng.random() < 0.4:
                    # via / pad centred on the joint, sometimes with the cap radius
                    r = rng.choice((width // 2, width // 2, 300_000 + 200_000, 400_000 + 200_000))
                    holes.append(cap_circle(x1, y1, r, kicad_seg_count(r, 5000)))
                x, y = x1, y1
        # pad arrays on a grid (identical shapes, touching or overlapping clearances)
        for _ in range(rng.randint(0, 4)):
            px, py = rng.randint(0, w // mm) * mm, rng.randint(0, h // mm) * mm
            pitch = rng.choice((500_000, 800_000, 1_000_000, 1_270_000, 2_540_000))
            a, b = rng.choice((250_000, 400_000, 500_000, 635_000)), rng.choice((250_000, 500_000, 1_000_000))
            horizontal = rng.random() < 0.5
            for k in range(rng.randint(2, 12)):
                cx = px + (k * pitch if horizontal else 0)
                cy = py + (0 if horizontal else k * pitch)
                kind = it % 3
                if kind == 0:
                    holes.append(rect(cx - a, cy - b, cx + a, cy + b))
                elif kind == 1:
                    r = min(a, b) // 2
                    holes.append(roundrect(cx - a, cy - b, cx + a, cy + b, r, max(2, kicad_seg_count(r, 5000) // 4)))
                else:
                    holes.append(cap_circle(cx, cy, a, kicad_seg_count(a, 5000)))
        # thermal relief: clearance ring minus four spokes through the pad centre
        spokes = []
        for _ in range(rng.randint(0, 5)):
            cx, cy = rng.randint(2, w // mm - 2) * mm, rng.randint(2, h // mm - 2) * mm
            r = rng.choice((800_000, 1_000_000, 1_500_000))
            holes.append(cap_circle(cx, cy, r, kicad_seg_count(r, 5000)))
            sw = rng.choice((200_000, 250_000, 500_000))
            spokes.append(rect(cx - 2 * r, cy - sw // 2, cx + 2 * r, cy + sw // 2))
            spokes.append(rect(cx - sw // 2, cy - 2 * r, cx + sw // 2, cy + 2 * r))
        emit("kicad_coincident", "BOOL", (CT_DIFFERENCE, FR_NONZERO, 1), zone, holes)
        emit("kicad_coincident", "BOOL", (CT_UNION, FR_NONZERO, 1), holes, [])
        emit("kicad_coincident", "BOOL", (CT_UNION, FR_NONZERO, 1), holes, spokes)
        emit("kicad_coincident", "BOOL", (CT_DIFFERENCE, FR_NONZERO, 1), holes, spokes)
        emit("kicad_coincident", "BOOL", (CT_INTERSECTION, FR_NONZERO, 1), zone + spokes, holes)
        emit("kicad_coincident", "BOOL", (CT_XOR, FR_NONZERO, 1), zone, holes + spokes)
        # the same shapes inflated together (clearance + zone min-width passes)
        for (jt, ml) in ((JT_ROUND, 2.0), (JT_MITER, 2.0), (JT_SQUARE, 2.0)):
            amount = rng.choice((-125_000, 125_000, 200_000, -50_000, 500_000))
            emit("kicad_coincident", "OFFSET",
                 (jt, ET_POLYGON, bits(ml), bits(kicad_arc_tol(amount)), bits(amount)),
                 groups=[[h] for h in holes[:40]])


def gen_degenerate():
    sq = rect(0, 0, 10, 10)
    cases = [
        ("empty_both", [], []),
        ("empty_clip", [sq], []),
        ("empty_subj", [], [sq]),
        ("one_point", [[(1, 1)]], [sq]),
        ("two_points", [[(1, 1), (5, 5)]], [sq]),
        ("all_same_point", [[(3, 3), (3, 3), (3, 3), (3, 3)]], [sq]),
        ("collinear_only", [[(0, 0), (5, 0), (10, 0), (20, 0)]], [sq]),
        ("zero_area_spike", [[(0, 0), (10, 0), (0, 0), (0, 10)]], [sq]),
        ("dup_points", [[(0, 0), (0, 0), (10, 0), (10, 0), (10, 10), (10, 10), (0, 10), (0, 0)]], [sq]),
        ("identical", [sq], [sq]),
        ("identical_reversed", [sq], [list(reversed(sq))]),
        ("identical_rotated", [sq], [sq[2:] + sq[:2]]),
        ("shared_edge", [sq], [rect(10, 0, 20, 10)]),
        ("shared_edge_partial", [sq], [rect(10, 3, 20, 7)]),
        ("shared_edge_overhang", [sq], [rect(10, -5, 20, 15)]),
        ("touch_corner", [sq], [rect(10, 10, 20, 20)]),
        ("touch_vertex_on_edge", [sq], [[(10, 5), (20, 0), (20, 10)]]),
        ("inside_touch_edge", [sq], [rect(0, 2, 5, 8)]),
        ("inside_touch_two_edges", [sq], [rect(0, 0, 5, 5)]),
        ("contained", [sq], [rect(2, 2, 8, 8)]),
        ("contains", [rect(2, 2, 8, 8)], [sq]),
        ("disjoint", [sq], [rect(20, 20, 30, 30)]),
        ("bowtie", [[(0, 0), (10, 10), (10, 0), (0, 10)]], [rect(2, -2, 8, 12)]),
        ("bowtie_self", [[(0, 0), (10, 10), (10, 0), (0, 10)]], []),
        ("figure8_vertex", [[(0, 0), (5, 5), (10, 0), (10, 10), (5, 5), (0, 10)]], []),
        ("collinear_verts", [[(0, 0), (5, 0), (10, 0), (10, 5), (10, 10), (5, 10), (0, 10), (0, 5)]], [rect(5, 5, 15, 15)]),
        ("spike_out", [[(0, 0), (10, 0), (10, 5), (20, 5), (10, 5), (10, 10), (0, 10)]], [rect(5, 5, 15, 15)]),
        ("spike_in", [[(0, 0), (10, 0), (10, 5), (5, 5), (10, 5), (10, 10), (0, 10)]], []),
        ("hole_touching_outline", [sq, list(reversed(rect(0, 3, 5, 7)))], []),
        ("hole_touching_vertex", [sq, list(reversed([(0, 0), (5, 3), (3, 5)]))], []),
        ("hole_equals_outline", [sq, list(reversed(sq))], [rect(5, 5, 15, 15)]),
        ("two_holes_touching", [rect(0, 0, 20, 20), list(reversed(rect(2, 2, 10, 10))), list(reversed(rect(10, 10, 18, 18)))], []),
        ("two_holes_shared_edge", [rect(0, 0, 20, 20), list(reversed(rect(2, 2, 10, 10))), list(reversed(rect(10, 2, 18, 10)))], []),
        ("island_touching_hole", [rect(0, 0, 20, 20), list(reversed(rect(2, 2, 18, 18))), rect(2, 2, 6, 6)], []),
        ("checker", [rect(0, 0, 10, 10), rect(10, 10, 20, 20), rect(20, 0, 30, 10)], [rect(10, 0, 20, 10), rect(0, 10, 10, 20), rect(20, 10, 30, 20)]),
        ("checker_self", [rect(0, 0, 10, 10), rect(10, 10, 20, 20), rect(20, 0, 30, 10), rect(0, 20, 10, 30), rect(20, 20, 30, 30)], []),
        ("overlap_twice", [sq, sq], [rect(5, 5, 15, 15)]),
        ("overlap_opposite", [sq, list(reversed(sq))], [rect(5, 5, 15, 15)]),
        ("slit", [[(0, 0), (10, 0), (10, 10), (5, 10), (5, 5), (5, 10), (0, 10)]], []),
        ("near_parallel", [[(0, 0), (1000000, 1), (1000000, 3), (0, 2)]], [[(0, 1), (1000000, 0), (1000000, 2), (0, 3)]]),
        ("tiny_triangle", [[(0, 0), (1, 0), (0, 1)]], [[(0, 0), (1, 1), (0, 1)]]),
        ("unit_squares", [rect(0, 0, 1, 1), rect(1, 0, 2, 1), rect(0, 1, 1, 2)], [rect(1, 1, 2, 2)]),
        ("horz_overlap", [[(0, 0), (10, 0), (10, 2), (0, 2)], [(5, 2), (15, 2), (15, 4), (5, 4)]], [[(2, 2), (8, 2), (8, -2), (2, -2)]]),
        ("t_junction", [rect(0, 0, 30, 10)], [rect(10, 10, 20, 30)]),
        ("cross", [rect(0, 10, 30, 20)], [rect(10, 0, 20, 30)]),
        ("diamond_in_square", [sq], [[(5, 0), (10, 5), (5, 10), (0, 5)]]),
        ("octagon_touch", [circle(0, 0, 100, 8)], [circle(200, 0, 100, 8)]),
        ("i32_extremes", [rect(-2147483648, -2147483648, 2147483647, 2147483647)], [[(-2147483648, 0), (0, -2147483648), (2147483647, 0), (0, 2147483647)]]),
        ("i32_thin", [[(-2147483648, -2147483648), (2147483647, 2147483646), (2147483647, 2147483647)]], [[(-2147483648, 2147483647), (2147483647, -2147483648), (2147483647, -2147483647)]]),
        ("kicad_arc_seam", [circle(0, 0, 1000000, 32, math.pi / 32)], [circle(1000000, 0, 1000000, 32, math.pi / 32)]),
        ("many_collinear_overlaps", [rect(0, 0, 10, 10), rect(3, 0, 13, 10), rect(6, 0, 16, 10), rect(9, 0, 19, 10)], [rect(1, 0, 4, 10), rect(7, 0, 17, 10)]),
    ]
    for (name, s, c) in cases:
        bool_cases("degenerate/" + name, s, c)
        if name in ("collinear_verts", "shared_edge", "many_collinear_overlaps", "checker", "hole_touching_outline"):
            bool_cases("degenerate_nocollinear/" + name, s, c, pc=0)


def gen_upstream(path):
    """Upstream Clipper2 Tests/Polygons.txt."""
    try:
        text = open(path, "r", encoding="latin-1").read().replace("\r", "")
    except OSError:
        return
    ctmap = {"INTERSECTION": CT_INTERSECTION, "UNION": CT_UNION, "DIFFERENCE": CT_DIFFERENCE, "XOR": CT_XOR}
    frmap = {"EVENODD": FR_EVENODD, "NONZERO": FR_NONZERO, "POSITIVE": FR_POSITIVE, "NEGATIVE": FR_NEGATIVE}
    blocks = text.split("CAPTION:")[1:]
    for b in blocks:
        ct, fr = CT_UNION, FR_EVENODD
        subj, clip = [], []
        cur = None
        skip = False
        for line in b.split("\n")[1:]:
            line = line.strip()
            if not line:
                continue
            if line.startswith("CLIPTYPE:"):
                ct = ctmap.get(line.split(":")[1].strip(), None)
                if ct is None:
                    skip = True
            elif line.startswith("FILLRULE:"):
                fr = frmap.get(line.split(":")[1].strip(), FR_EVENODD)
            elif line.startswith("SOL_"):
                continue
            elif line == "SUBJECTS":
                cur = subj
            elif line == "SUBJECTS_OPEN":
                cur = None
                skip = True
            elif line == "CLIPS":
                cur = clip
            elif cur is not None:
                nums = [int(t) for t in line.replace(",", " ").split()]
                if len(nums) >= 2:
                    cur.append([(nums[i], nums[i + 1]) for i in range(0, len(nums) - 1, 2)])
        if skip:
            continue
        emit("upstream_polygons", "BOOL", (ct, fr, 1), subj, clip)


# ---------------------------------------------------------------- offset categories

def kicad_arc_tol(amount, err_max=5000):
    seg = kicad_seg_count(abs(amount), err_max)
    if seg < 6:
        seg = 6
    coeff = 1.0 - math.cos(math.pi / seg)
    return abs(amount) * coeff


def gen_offset():
    kicad_joins = ((JT_ROUND, 2.0), (JT_MITER, 2.0), (JT_MITER, 10.0), (JT_SQUARE, 2.0))
    # KiCad-realistic: polygons in nm, Inflate()/Deflate() amounts
    for _ in range(90):
        mm = 1_000_000
        groups = []
        for _ in range(rng.randint(1, 3)):
            kind = rng.random()
            cx, cy = rng.randint(-20 * mm, 20 * mm), rng.randint(-20 * mm, 20 * mm)
            if kind < 0.25:
                a, b = rng.randint(mm // 4, 5 * mm), rng.randint(mm // 4, 5 * mm)
                g = [rect(cx - a, cy - b, cx + a, cy + b)]
            elif kind < 0.45:
                r = rng.randint(100_000, 3 * mm)
                g = [circle(cx, cy, r, kicad_seg_count(r, 5000))]
            elif kind < 0.6:
                a, b = rng.randint(mm, 5 * mm), rng.randint(mm, 5 * mm)
                r = min(a, b) // rng.choice((2, 3, 5))
                g = [roundrect(cx - a, cy - b, cx + a, cy + b, r, max(2, kicad_seg_count(r, 5000) // 4))]
            elif kind < 0.8:
                g = [oriented(rnd_poly(rng.randint(3, 12), -10 * mm, 10 * mm), True)]
            else:
                a, b = rng.randint(3 * mm, 10 * mm), rng.randint(3 * mm, 10 * mm)
                g = [rect(cx - a, cy - b, cx + a, cy + b)]
                for _ in range(rng.randint(1, 3)):
                    hx, hy = cx + rng.randint(-a + mm, a - mm), cy + rng.randint(-b + mm, b - mm)
                    r = rng.randint(100_000, mm)
                    g.append(oriented(circle(hx, hy, r, kicad_seg_count(r, 5000)), False))
            groups.append(g)
        for (jt, ml) in kicad_joins:
            for sign in (1, -1):
                amount = sign * rng.choice((1, 10, 100, 2000, 25_000, 50_000, 100_000, 127_000, 200_000,
                                            254_000, 500_000, 1_000_000, 2_540_000,
                                            rng.randint(1, 3_000_000)))
                err = rng.choice((5000, 5000, 2000, 20_000))
                emit("kicad_inflate", "OFFSET",
                     (jt, ET_POLYGON, bits(ml), bits(kicad_arc_tol(amount, err)), bits(amount)), groups=groups)
    # KiCad OffsetLineChain: open path, EndType::Butt
    for _ in range(50):
        mm = 1_000_000
        n = rng.randint(2, 10)
        p = rnd_poly(n, -10 * mm, 10 * mm)
        if rng.random() < 0.5:
            # 45 degree routed track
            p = [(0, 0)]
            for _ in range(n):
                d = rng.choice(((1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)))
                ln = rng.randint(1, 50) * 100_000
                p.append((p[-1][0] + d[0] * ln, p[-1][1] + d[1] * ln))
        for (jt, ml) in kicad_joins:
            amount = rng.choice((50_000, 75_000, 100_000, 125_000, 250_000, 500_000, rng.randint(1000, 2_000_000)))
            emit("kicad_offset_line", "OFFSET",
                 (jt, ET_BUTT, bits(ml), bits(kicad_arc_tol(amount)), bits(amount)), groups=[[p]])
    # generic random polygons, every join type, Polygon end type
    for _ in range(70):
        g = rng.choice((10, 50, 1000, 100000))
        paths = [rnd_poly(rng.randint(3, 10), 0, g) for _ in range(rng.randint(1, 3))]
        for jt in (JT_SQUARE, JT_BEVEL, JT_ROUND, JT_MITER):
            for sign in (1, -1):
                delta = sign * rng.choice((1, 2, 3, g / 20, g / 7, g / 3, rng.uniform(0.5, g / 2)))
                ml = rng.choice((2.0, 2.0, 10.0, 1.0, 3.5))
                at = rng.choice((0.0, 0.0, 0.25, abs(delta) * (1 - math.cos(math.pi / rng.choice((8, 16, 32, 64))))))
                emit("rnd_offset", "OFFSET", (jt, ET_POLYGON, bits(ml), bits(at), bits(delta)), groups=[paths])
    # open paths, every end type
    for _ in range(40):
        g = rng.choice((10, 50, 1000, 100000))
        paths = [rnd_poly(rng.randint(1, 8), 0, g) for _ in range(rng.randint(1, 2))]
        for et in (ET_JOINED, ET_BUTT, ET_SQUARE, ET_ROUND):
            jt = rng.choice((JT_SQUARE, JT_BEVEL, JT_ROUND, JT_MITER))
            delta = rng.choice((1, 2, g / 20, g / 7, rng.uniform(0.5, g / 4)))
            at = rng.choice((0.0, 0.25, abs(delta) * (1 - math.cos(math.pi / rng.choice((8, 16, 32))))))
            emit("rnd_offset_open", "OFFSET", (jt, et, bits(2.0), bits(at), bits(delta)), groups=[paths])
    # degenerate offset inputs
    sq = rect(0, 0, 100, 100)
    degs = [
        [[(5, 5)]],
        [[(5, 5), (50, 5)]],
        [[(0, 0), (10, 0), (20, 0)]],
        [sq],
        [list(reversed(sq))],
        [sq, list(reversed(rect(40, 40, 60, 60)))],
        [sq, rect(100, 0, 200, 100)],
        [[(0, 0), (100, 0), (100, 100), (50, 100), (50, 50), (50, 100), (0, 100)]],
        [[(0, 0), (100, 100), (100, 0), (0, 100)]],
        [[(0, 0), (0, 0), (100, 0), (100, 0), (100, 100), (0, 100), (0, 0)]],
        [rect(0, 0, 3, 3)],
        [rect(0, 0, 1, 1)],
    ]
    for d in degs:
        for jt in (JT_SQUARE, JT_BEVEL, JT_ROUND, JT_MITER):
            for delta in (10.0, -10.0, 1.0, -2.0, 60.0, -60.0, 0.0):
                for et in (ET_POLYGON, ET_BUTT):
                    emit("degenerate_offset", "OFFSET", (jt, et, bits(2.0), bits(0.0), bits(delta)), groups=[d])


def gen_misc():
    # SimplifyPaths (KiCad calls it, but discards the result)
    for _ in range(150):
        g = rng.choice((10, 100, 10000, 2_000_000_000))
        paths = [rnd_poly(rng.randint(1, 30), 0, g) for _ in range(rng.randint(1, 3))]
        if rng.random() < 0.4:
            r = rng.randint(10, g // 2 + 10)
            paths = [circle(0, 0, r, rng.choice((8, 32, 200)))]
        eps = rng.choice((0.5, 1.0, 2.0, g / 50, g / 10))
        emit("simplify", "SIMPLIFY", (bits(eps), rng.choice((0, 1))), paths)
    # Area
    for _ in range(150):
        g = rng.choice((10, 1000, 2_000_000_000, 10 ** 18))
        emit("area", "AREA", (), [rnd_poly(rng.randint(0, 40), -g, g) for _ in range(rng.randint(1, 3))])
    # RectClip (not used by KiCad)
    for _ in range(250):
        g = rng.choice((20, 100, 10000))
        x0, y0 = rng.randint(0, g // 2), rng.randint(0, g // 2)
        r = (x0, y0, x0 + rng.randint(1, g // 2), y0 + rng.randint(1, g // 2))
        paths = [rnd_poly(rng.randint(3, 12), 0, g) for _ in range(rng.randint(1, 3))]
        emit("rectclip", "RECTCLIP", r, paths)
    # Minkowski (not used by KiCad)
    for _ in range(100):
        g = rng.choice((20, 1000))
        pattern = rnd_poly(rng.randint(3, 6), -g // 10 - 1, g // 10 + 1)
        path = rnd_poly(rng.randint(3, 10), 0, g)
        emit("minkowski", "MINKSUM", (rng.choice((0, 1)),), [pattern], [path])
        emit("minkowski", "MINKDIFF", (rng.choice((0, 1)),), [pattern], [path])


def main():
    gen_kicad_coincident()
    gen_bool()
    gen_degenerate()
    if len(sys.argv) > 2:
        gen_upstream(sys.argv[2])
    gen_offset()
    gen_misc()
    with open(sys.argv[1], "w") as f:
        f.write("\n".join(out))
        f.write("\n")
    cats = {}
    for c in out:
        h = c.split("\n", 1)[0].split()
        key = h[2].split("/")[0] + " " + h[3]
        cats[key] = cats.get(key, 0) + 1
    for k in sorted(cats):
        print("%-36s %d" % (k, cats[k]))
    print("total", len(out))


main()
