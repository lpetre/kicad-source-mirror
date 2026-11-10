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
#
# REV-FAB review harness (rust/docs/design/review-fab.md). Scratch output goes to
# rust/target/rev-fab/work; the reference kicad-cli always runs on copies.
"""drill_matrix.py MODE BOARD... : MODE full = full factorial of the Excellon options; MODE other = gerber/map/report sets."""
import itertools, os, subprocess, sys
T = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fabcmp.py")
mode, boards = sys.argv[1], sys.argv[2:]
sets = []
if mode == "full":
    for z, u, o, m, h, s, ov in itertools.product(("decimal", "suppressleading", "suppresstrailing", "keep"), ("mm", "in"), ("absolute", "plot"), (0, 1), (0, 1), (0, 1), ("alternate", "route")):
        opts = ["--excellon-zeros-format", z, "-u", u, "--drill-origin", o, "--excellon-oval-format", ov]
        if m: opts.append("--excellon-mirror-y")
        if h: opts.append("--excellon-min-header")
        if s: opts.append("--excellon-separate-th")
        sets.append(("%s_%s_%s_m%d_h%d_s%d_%s" % (z[:8], u, o[:3], m, h, s, ov[:3]), opts))
elif mode == "pair":
    import random
    random.seed(7)
    allc = list(itertools.product(("decimal", "suppressleading", "suppresstrailing", "keep"), ("mm", "in"), ("absolute", "plot"), (0, 1), (0, 1), (0, 1), ("alternate", "route")))
    for z, u, o, m, h, s, ov in random.sample(allc, 24):
        opts = ["--excellon-zeros-format", z, "-u", u, "--drill-origin", o, "--excellon-oval-format", ov]
        if m: opts.append("--excellon-mirror-y")
        if h: opts.append("--excellon-min-header")
        if s: opts.append("--excellon-separate-th")
        sets.append(("%s_%s_%s_m%d_h%d_s%d_%s" % (z[:8], u, o[:3], m, h, s, ov[:3]), opts))
else:
    for p, o in itertools.product(("5", "6"), ("absolute", "plot")):
        sets.append(("gbr_p%s_%s" % (p, o[:3]), ["--format", "gerber", "--gerber-precision", p, "--drill-origin", o]))
        sets.append(("gbr_p%s_%s_tent" % (p, o[:3]), ["--format", "gerber", "--gerber-precision", p, "--drill-origin", o, "--generate-tenting"]))
    sets.append(("gbr_sep", ["--format", "gerber", "--excellon-separate-th", "--excellon-mirror-y", "-u", "in"]))
    sets.append(("exc_tent", ["--generate-tenting"]))
    sets.append(("exc_report", ["--generate-report"]))
    sets.append(("exc_report_path", ["--generate-report", "--report-path", "@OUT@rep/my report.txt"]))
    sets.append(("exc_report_sep", ["--generate-report", "--excellon-separate-th", "-u", "in"]))
    sets.append(("gbr_report", ["--format", "gerber", "--generate-report"]))
    for mf in ("gerberx2", "svg", "pdf", "ps", "dxf"):
        sets.append(("map_" + mf, ["--generate-map", "--map-format", mf]))
    sets.append(("gbr_map_gbr", ["--format", "gerber", "--generate-map", "--map-format", "gerberx2"]))
    sets.append(("bad_zeros", ["--excellon-zeros-format", "nope"]))
    sets.append(("bad_units", ["-u", "furlong"]))
    sets.append(("bad_prec", ["--format", "gerber", "--gerber-precision", "7"]))
ok = fail = 0; shown = 0
for b in boards:
    bf = []
    for tag, opts in sets:
        p = subprocess.run([sys.executable, T, "--show", "10" if shown < int(os.environ.get("NSHOW", "4")) else "0", "drill", tag, b, "--"] + opts, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        out = p.stdout.decode("utf-8", "replace")
        if out.startswith("OK"): ok += 1
        else:
            fail += 1; bf.append(tag)
            if shown < int(os.environ.get("NSHOW", "4")):
                shown += 1; print(out.rstrip()[:1500])
    print("BOARD %s: sets=%d fail=%d %s" % (os.path.basename(b), len(sets), len(bf), " ".join(bf)[:1200]))
    sys.stdout.flush()
print("TOTAL ok=%d fail=%d" % (ok, fail))
