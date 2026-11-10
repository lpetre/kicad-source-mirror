#!/usr/bin/env python3
# Package SET-3: for every .kicad_pro under qa/ and demos/ with a sibling .kicad_pcb, copies both
# (and a .kicad_dru if present) to a scratch directory, runs `pcb-dump board --sections board`
# (QA KI_TEST::LoadBoard path: the board is bound to its project) and extracts the net class
# table (ds.netclass.*), the pattern assignments (ds.netclass_pattern.*), the label assignments
# (ds.netclass_label.*), and the effective netclass name of every net (net.<code>.name +
# effective_netclass.<code>). Writes tests/golden/net_settings_json_pcbdump.txt, replayed by
# tests/net_settings_json.rs (`pcb_dump_netclasses`): one `nc` line per class with every field,
# and count + FNV-1a hash of the pattern list, the label list and the sorted "net<TAB>class"
# lines, followed by the net names (`net` lines). Boards with legacy (net_class ...) sections are
# marked `legacy` (BOARD::SetProject replaces the project's classes by the board's).
# Usage: tests/oracle/net_settings_json_pcbdump.py   (any directory; about 10 minutes)
import glob, os, shutil, subprocess, sys, tempfile

here = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
root = os.path.abspath(os.path.join(here, "..", "..", ".."))
out_path = os.path.join(here, "tests", "golden", "net_settings_json_pcbdump.txt")
dump = os.environ.get("PCB_DUMP", "/home/user/kicad-oracle/bin/pcb-dump")

FIELDS = ["name", "is_default", "priority", "clearance", "track_width", "via_diameter", "via_drill",
          "uvia_diameter", "uvia_drill", "diff_pair_width", "diff_pair_gap", "diff_pair_via_gap",
          "wire_width", "bus_width", "line_style", "pcb_color", "schematic_color", "tuning_profile",
          "description"]


def fnv(s):
    h = 0xcbf29ce484222325
    for b in s.encode("utf-8", "surrogateescape"):
        h ^= b
        h = (h * 0x100000001b3) & 0xffffffffffffffff
    return "%016x" % h


def unq(v, i=0):
    """Parses one quoted pcb-dump string starting at v[i] (str); returns (text, next index)."""
    b = v.encode("utf-8", "surrogateescape")
    j = len(v[:i].encode("utf-8", "surrogateescape"))
    assert b[j] == 0x22, v
    j += 1
    out = bytearray()
    while b[j] != 0x22:
        if b[j] == 0x5c:
            c = chr(b[j + 1])
            if c == "x":
                out.append(int(b[j + 2:j + 4], 16))
                j += 4
                continue
            out += {"\\": b"\\", '"': b'"', "n": b"\n", "r": b"\r", "t": b"\t"}[c]
            j += 2
            continue
        out.append(b[j])
        j += 1
    return out.decode("utf-8", "surrogateescape"), len(b[:j + 1].decode("utf-8", "surrogateescape"))


def value(v):
    if v.startswith('"'):
        return unq(v)[0]
    if v == "true":
        return "1"
    if v == "false":
        return "0"
    if v == "none":
        return "~"
    return v


def strings(v):
    res, i = [], 0
    while i < len(v):
        if v[i] == '"':
            s, i = unq(v, i)
            res.append(s)
        else:
            i += 1
    return res


pros = sorted(glob.glob(os.path.join(root, "qa", "**", "*.kicad_pro"), recursive=True)
              + glob.glob(os.path.join(root, "demos", "**", "*.kicad_pro"), recursive=True))
lines = []
for pro in pros:
    pcb = pro[:-len(".kicad_pro")] + ".kicad_pcb"
    if not os.path.exists(pcb):
        continue
    rel = os.path.relpath(pro, root)
    with tempfile.TemporaryDirectory(dir=os.environ.get("TMPDIR")) as tmp:
        base = os.path.basename(pro)[:-len(".kicad_pro")]
        for ext in (".kicad_pro", ".kicad_pcb", ".kicad_dru"):
            src = pro[:-len(".kicad_pro")] + ext
            if os.path.exists(src):
                shutil.copy(src, os.path.join(tmp, base + ext))
        try:
            p = subprocess.run([dump, "board", "--sections", "board", os.path.join(tmp, base + ".kicad_pcb")],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=900)
        except subprocess.TimeoutExpired:
            print("timeout", rel, file=sys.stderr)
            continue
    classes, order, patterns, labels, names, eff = {}, [], [], [], {}, {}
    for raw in p.stdout.decode("utf-8", "surrogateescape").split("\n"):
        if not raw.startswith("board|board|"):
            continue
        key, _, v = raw[len("board|board|"):].partition("=")
        if key.startswith("ds.netclass."):
            cls, _, field = key[len("ds.netclass."):].rpartition(".")
            if cls not in classes:
                classes[cls] = {}
                order.append(cls)
            classes[cls][field] = value(v)
        elif key.startswith("ds.netclass_pattern."):
            patterns.append((int(key.rpartition(".")[2]), strings(v)))
        elif key.startswith("ds.netclass_label."):
            labels.append([key[len("ds.netclass_label."):]] + strings(v))
        elif key.startswith("net.") and key.endswith(".name"):
            names[int(key.split(".")[1])] = value(v)
        elif key.startswith("effective_netclass."):
            eff[int(key.split(".")[1])] = value(v)
    if not classes:
        print("no netclasses", rel, file=sys.stderr)
        continue
    legacy = b"(net_class " in open(pcb, "rb").read()
    lines.append("proj\t" + rel + "\t" + os.path.relpath(pcb, root) + ("\tlegacy" if legacy else ""))
    for cls in order:
        lines.append("nc\t" + "\t".join(classes[cls].get(f, "?") for f in FIELDS))
    pat = [p[1][0] + "\t" + p[1][1] for p in sorted(patterns)]
    lines.append("pat\t%d\t%s" % (len(pat), fnv("".join(l + "\n" for l in pat))))
    lab = ["\t".join(l) for l in labels]
    lines.append("lab\t%d\t%s" % (len(lab), fnv("".join(l + "\n" for l in lab))))
    effl = sorted((names[c] + "\t" + eff[c] for c in eff), key=lambda s: s.encode("utf-8", "surrogateescape"))
    lines.append("eff\t%d\t%s" % (len(effl), fnv("".join(l + "\n" for l in effl))))
    # The net names (the Rust board parser cannot supply them yet): one line per net, the
    # effective class only where it is not "Default" (both sides check the hash above).
    for l in effl:
        n, _, c = l.partition("\t")
        lines.append("net\t" + n + ("\t" + c if c != "Default" else ""))
    print(rel, len(classes), len(pat), len(lab), len(effl), file=sys.stderr)

open(out_path, "w", encoding="utf-8", errors="surrogateescape").write("\n".join(lines) + "\n")
print(len(lines), "lines", file=sys.stderr)
