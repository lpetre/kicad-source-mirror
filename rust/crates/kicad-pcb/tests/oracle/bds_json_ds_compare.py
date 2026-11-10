#!/usr/bin/env python3
# Package SET-4: compares the design settings a board gets from its project file, as the C++
# reference prints them (`pcb-dump board --sections board`, QA load path, which binds the
# sibling .kicad_pro) and as the Rust port computes them (board parser + SET-4 loader, test
# `project::board_design_settings_json::tests::pcb_dump_ds_lines`).
#
# Only the `ds.*` fields written by the `board.design_settings` parameters are compared (net
# classes, stackup, router state and derived getters belong to other packages); meander
# settings are not printed by the Rust dump and are covered by bds_json_oracle.cpp instead.
#
# Usage: tests/oracle/bds_json_ds_compare.py   (needs /home/user/kicad-oracle/bin/pcb-dump)
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.dirname(os.path.dirname(HERE))
RUST = os.path.dirname(os.path.dirname(CRATE))
KICAD = os.path.dirname(RUST)
PCB_DUMP = "/home/user/kicad-oracle/bin/pcb-dump"
SKIP_LARGE = ("jetson", "vme-wren", "openair", "tiny_tapeout")

PREFIXES = (
    "ds.track_width_list", "ds.vias_dimensions_list", "ds.diff_pair_dimensions_list",
    "ds.via_stack_preset.", "ds.teardrops.", "ds.min_", "ds.track_min_width",
    "ds.vias_min_", "ds.micro_vias_min_", "ds.copper_edge_clearance", "ds.hole_clearance",
    "ds.hole_to_hole_min", "ds.silk_clearance", "ds.drc_severity.", "ds.drc_exclusions",
    "ds.zone_keep_external_fillets", "ds.max_error", "ds.solder_mask_to_copper_clearance",
    "ds.layer_class.", "ds.dimension.", "ds.style_fp_", "ds.use_height_for_length_calcs",
    "ds.default_zone_settings.",
)
ZONE_FIELDS = ("clearance", "min_thickness", "fill_mode", "hatch_thickness", "hatch_gap",
               "hatch_orientation", "hatch_smoothing_level", "hatch_smoothing_value",
               "border_display_style", "border_hatch_pitch", "thermal_relief_gap",
               "thermal_relief_spoke_width", "pad_connection", "corner_smoothing",
               "corner_radius")


def wanted(field):
    if not field.startswith(PREFIXES):
        return False
    if field.startswith("ds.default_zone_settings."):
        return field.split(".", 2)[2] in ZONE_FIELDS
    return True


def parse(text):
    out, cur = {}, None
    for line in text.splitlines():
        if line.startswith("file|"):
            cur = out.setdefault(line[5:], {})
        elif cur is not None and line.startswith("board|board|ds."):
            field, _, value = line[len("board|board|"):].partition("=")
            if wanted(field):
                cur[field] = value
        elif cur is not None and line.startswith("meta|"):
            cur["__meta"] = line
    return out


def main():
    boards = []
    for base in ("qa/data", "demos"):
        for root, _, files in os.walk(os.path.join(KICAD, base)):
            for f in files:
                if f.endswith(".kicad_pro"):
                    pcb = os.path.join(root, f[:-4] + "_pcb")
                    if os.path.exists(pcb) and not any(s in pcb for s in SKIP_LARGE):
                        boards.append(pcb)
    boards.sort()

    work = tempfile.mkdtemp(dir=os.path.join(RUST, "target"))
    try:
        cxx_text = []
        for i, pcb in enumerate(boards):
            d = os.path.join(work, str(i))
            os.makedirs(d)
            shutil.copy(pcb, d)
            shutil.copy(pcb[:-10] + ".kicad_pro", d)
            env = dict(os.environ, HOME=os.path.join(work, "home"))
            r = subprocess.run([PCB_DUMP, "board", "--sections", "board",
                                os.path.join(d, os.path.basename(pcb))],
                               capture_output=True, text=True, env=env)
            cxx_text.append("file|" + pcb)
            cxx_text.extend(l for l in r.stdout.splitlines() if l.startswith("board|board|ds."))

        listing = os.path.join(work, "boards.txt")
        open(listing, "w").write("\n".join(boards) + "\n")
        rust_out = os.path.join(work, "rust.txt")
        env = dict(os.environ, SET4_BOARDS=listing, SET4_OUT=rust_out)
        subprocess.run(["cargo", "test", "-q", "-p", "kicad-pcb", "--features", "dump", "--lib",
                        "pcb_dump_ds_lines", "--", "--ignored"], cwd=RUST, env=env, check=True,
                       stdout=subprocess.DEVNULL)
        rust = parse(open(rust_out).read())
        cxx = parse("\n".join(cxx_text))
    finally:
        shutil.rmtree(work)

    compared = legacy = 0
    diffs = {}
    for pcb in boards:
        r, c = rust.get(pcb, {}), cxx.get(pcb, {})
        meta = r.get("__meta", "")
        if not c or not meta:
            continue
        if "legacy=true" in meta:
            legacy += 1
            continue
        compared += 1
        for k in sorted(set(c) | set(r)):
            if k != "__meta" and c.get(k) != r.get(k):
                diffs.setdefault(k, []).append((pcb, c.get(k), r.get(k)))

    print(f"boards: {len(boards)}, compared: {compared}, legacy setup skipped: {legacy}")
    for k, v in sorted(diffs.items()):
        print(f"{k}: {len(v)} boards, e.g. {os.path.relpath(v[0][0], KICAD)} cxx={v[0][1]} rust={v[0][2]}")
    return 1 if diffs else 0


if __name__ == "__main__":
    sys.exit(main())
