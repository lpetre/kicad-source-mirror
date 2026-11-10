#!/usr/bin/env python3
# STAB-CN step 4: compares the hashed `item|KEY|net=` / `netname=` lines of two pcb-dump
# outputs (reference, Rust) per item type. Prints one line:
#   ref_items=N rs_items=N net_match=N net_mismatch=N netname_match=N netname_mismatch=N
#   missing=N extra=N [mismatch=TYPE:KEY:field,...] [rs_error=...] [ref_error=...]
# Types are recovered from the hashed `type=` values (FNV-1a 64 of the type name).
import sys

TYPES = ["PCB_TRACE_T", "PCB_VIA_T", "PCB_ARC_T", "PCB_PAD_T", "PCB_ZONE_T", "PCB_SHAPE_T",
         "PCB_FOOTPRINT_T", "PCB_FIELD_T", "PCB_TEXT_T", "PCB_TEXTBOX_T", "PCB_TABLE_T",
         "PCB_TABLECELL_T", "PCB_DIMENSION_T", "PCB_DIM_ALIGNED_T", "PCB_DIM_LEADER_T",
         "PCB_DIM_CENTER_T", "PCB_DIM_RADIAL_T", "PCB_DIM_ORTHOGONAL_T", "PCB_REFERENCE_IMAGE_T",
         "PCB_GROUP_T", "PCB_GENERATOR_T", "PCB_MARKER_T", "PCB_TARGET_T", "PCB_POINT_T",
         "PCB_BARCODE_T"]


def fnv(s):
    h = 0xcbf29ce484222325
    for b in s.encode():
        h ^= b
        h = (h * 0x100000001b3) & 0xffffffffffffffff
    return "#%016x" % h


BY_HASH = {fnv(t): t for t in TYPES}


def parse(path):
    items, errors = {}, []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith("error|"):
                errors.append(line[:160])
                continue
            parts = line.split("|", 2)
            if len(parts) < 3 or "=" not in parts[2]:
                continue
            field, value = parts[2].split("=", 1)
            items.setdefault(parts[1], {})[field] = value
    return items, errors


def main():
    ref, ref_err = parse(sys.argv[1])
    rs, rs_err = parse(sys.argv[2])
    c = dict(net_match=0, net_mismatch=0, netname_match=0, netname_mismatch=0, missing=0, extra=0)
    bad, by_type = [], {}
    for key, f in ref.items():
        t = BY_HASH.get(f.get("type", ""), f.get("type", "?"))
        if "net" not in f:
            continue
        g = rs.get(key)
        if g is None or "net" not in g:
            c["missing"] += 1
            bad.append("%s:%s:missing" % (t, key))
            continue
        for field in ("net", "netname"):
            if field not in f:
                continue
            if g.get(field) == f[field]:
                c[field + "_match"] += 1
                by_type[t] = by_type.get(t, 0) + (field == "net")
            else:
                c[field + "_mismatch"] += 1
                bad.append("%s:%s:%s" % (t, key, field))
    for key, g in rs.items():
        if "net" in g and (key not in ref or "net" not in ref[key]):
            c["extra"] += 1
            bad.append("%s:%s:extra" % (BY_HASH.get(g.get("type", ""), "?"), key))
    out = ["ref_items=%d" % len(ref), "rs_items=%d" % len(rs)]
    out += ["%s=%d" % kv for kv in c.items()]
    out.append("types=" + ",".join("%s:%d" % (t[4:-2], n) for t, n in sorted(by_type.items())))
    if bad:
        out.append("bad=" + ",".join(bad[:20]))
    if rs_err:
        out.append("rs_error=" + rs_err[0].replace(" ", "_"))
    if ref_err:
        out.append("ref_error=" + ref_err[0].replace(" ", "_"))
    print(" ".join(out))


main()
