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

"""DRC-28: synthetic boards for the track angle, track segment length and via stack providers
and their reference goldens.

Writes each case's board (plus `.kicad_dru` and `.kicad_pro` where the case needs them) under
`tests/data/drc28/` and runs the reference `kicad-cli pcb drc --format json --severity-all
--units mm` on a copy; `tests/golden/drc28/<name>.txt` holds the entries of the DRC-28 error
types in report order (severity, type, description, then uuid, description, x, y per item,
tab separated).

The via stack cases rebuild the boards the C++ test `qa/tests/pcbnew/drc/test_drc_via_stack.cpp`
makes in code (same layer counts, stackups, vias, pads and rules; the C++ test case is named
per case below), so its expected counts can be checked against the reference and the port.

Run with `python3 -I`; the reference runs sequentially, on copies.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data", "drc28")
GOLDEN = os.path.join(HERE, "..", "golden", "drc28")

TYPES = ("track_angle", "track_segment_length", "malformed_microvia_stack_span",
         "microvia_stack_not_filled", "microvia_stack_depth", "hole_aspect_ratio",
         "microvia_crosses_core")

CU = {2: ["F.Cu", "B.Cu"],
      4: ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"],
      6: ["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"]}
CU_ID = {"F.Cu": 0, "B.Cu": 2, "In1.Cu": 4, "In2.Cu": 6, "In3.Cu": 8, "In4.Cu": 10}

_uuid = [0]


def uid():
    _uuid[0] += 1
    return "00000000-0000-0000-0028-%012x" % _uuid[0]


def stackup(ncu, copper, dielectrics):
    """A stackup section: `copper` maps a layer name to its thickness (default 0.035),
    `dielectrics` is one entry per dielectric: (type, [thickness per sublayer])."""
    out = ['\t\t(stackup\n',
           '\t\t\t(layer "F.SilkS" (type "Top Silk Screen"))\n',
           '\t\t\t(layer "F.Paste" (type "Top Solder Paste"))\n',
           '\t\t\t(layer "F.Mask" (type "Top Solder Mask") (thickness 0.01))\n']
    names = CU[ncu]

    for i, name in enumerate(names):
        out.append('\t\t\t(layer "%s" (type "copper") (thickness %s))\n'
                   % (name, copper.get(name, 0.035)))

        if i < len(names) - 1:
            kind, subs = dielectrics[i]
            body = '(type "%s") (thickness %s) (material "FR4") (epsilon_r 4.5) ' \
                   '(loss_tangent 0.02)' % (kind, subs[0])

            for t in subs[1:]:
                body += ' addsublayer (thickness %s) (material "FR4") (epsilon_r 4.5) ' \
                        '(loss_tangent 0.02)' % t

            out.append('\t\t\t(layer "dielectric %d" %s)\n' % (i + 1, body))

    out += ['\t\t\t(layer "B.Mask" (type "Bottom Solder Mask") (thickness 0.01))\n',
            '\t\t\t(layer "B.Paste" (type "Bottom Solder Paste"))\n',
            '\t\t\t(layer "B.SilkS" (type "Bottom Silk Screen"))\n',
            '\t\t\t(copper_finish "None")\n',
            '\t\t\t(dielectric_constraints no)\n',
            '\t\t)\n']
    return "".join(out)


def header(ncu, stack=None):
    layers = "".join('\t\t(%d "%s" signal)\n' % (CU_ID[n], n) for n in CU[ncu])
    return """(kicad_pcb
\t(version 20260206)
\t(generator "pcbnew")
\t(generator_version "10.0")
\t(general
\t\t(thickness 1.6)
\t\t(legacy_teardrops no)
\t)
\t(paper "A4")
\t(layers
%s\t\t(5 "F.SilkS" user "F.Silkscreen")
\t\t(7 "B.SilkS" user "B.Silkscreen")
\t\t(1 "F.Mask" user)
\t\t(3 "B.Mask" user)
\t\t(13 "F.Paste" user)
\t\t(15 "B.Paste" user)
\t\t(25 "Edge.Cuts" user)
\t\t(31 "F.CrtYd" user "F.Courtyard")
\t\t(29 "B.CrtYd" user "B.Courtyard")
\t)
\t(setup
%s\t\t(pad_to_mask_clearance 0)
\t)
\t(net 0 "")
\t(net 1 "N1")
\t(net 2 "N2")
""" % (layers, stack or "")


def seg(x1, y1, x2, y2, net="N1", layer="F.Cu", w=0.25):
    return '\t(segment (start %s %s) (end %s %s) (width %s) (layer "%s") (net "%s") ' \
           '(uuid "%s"))\n' % (x1, y1, x2, y2, w, layer, net, uid())


def arc(x1, y1, xm, ym, x2, y2, net="N1", layer="F.Cu", w=0.25):
    return '\t(arc (start %s %s) (mid %s %s) (end %s %s) (width %s) (layer "%s") ' \
           '(net "%s") (uuid "%s"))\n' % (x1, y1, xm, ym, x2, y2, w, layer, net, uid())


def via(x, y, top, bot, drill=0.15, size=None, kind="micro", filled=True, net="", u=None):
    size = size if size is not None else round(drill + 0.15, 6)
    kw = "" if kind == "through" else kind + " "
    fill = "" if filled is None else "\t\t(filling %s)\n" % ("yes" if filled else "no")
    return '\t(via %s(at %s %s) (size %s) (drill %s) (layers "%s" "%s")\n%s' \
           '\t\t(net "%s") (uuid "%s"))\n' % (kw, x, y, size, drill, top, bot, fill, net,
                                             u or uid())


def via_stack(start, end, members, x=0, y=0, size=0.3, drill=0.15):
    return """\t(generated
\t\t(uuid "%s")
\t\t(type via_stack)
\t\t(name "Microvia Stack")
\t\t(layer "%s")
\t\t(capped no)
\t\t(end_layer "%s")
\t\t(filled yes)
\t\t(origin (xy %s %s))
\t\t(pitch 0)
\t\t(preset "")
\t\t(start_layer "%s")
\t\t(style "stacked")
\t\t(use_netclass no)
\t\t(via_drill %s)
\t\t(via_size %s)
\t\t(members %s)
\t)
""" % (uid(), start, end, x, y, start, drill, size, " ".join('"%s"' % m for m in members))


def pth_pad(x, y, dx, dy, npth=False):
    """`makePthPad`: an oval PTH pad (drill + 0.3 mm) in its own footprint."""
    shape = "circle" if dx == dy else "oval"
    drill = "(drill %s)" % dx if dx == dy else "(drill oval %s %s)" % (dx, dy)
    kind = "np_thru_hole" if npth else "thru_hole"
    layers = '"*.Cu" "*.Mask"' if not npth else '"*.Cu" "*.Mask"'
    return """\t(footprint "drc28:P" (layer "F.Cu") (uuid "%s") (at %s %s)
\t\t(pad "1" %s %s (at 0 0) (size %s %s) %s (layers %s) (uuid "%s"))
\t)
""" % (uid(), x, y, kind, shape, round(dx + 0.3, 6), round(dy + 0.3, 6), drill, layers, uid())


def smd_pad(x, y, net="N1"):
    return """\t(footprint "drc28:S" (layer "F.Cu") (uuid "%s") (at %s %s)
\t\t(pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Mask") (net "%s") (uuid "%s"))
\t)
""" % (uid(), x, y, net, uid())


def rule(constraint, condition=None, severity=None, name="drc28"):
    s = '(version 1)\n(rule "%s"\n' % name

    if condition:
        s += '    (condition "%s")\n' % condition

    if severity:
        s += '    (severity %s)\n' % severity

    return s + "    (constraint %s)\n)\n" % constraint


def pro(name, ignored):
    sev = ",\n".join('        "%s": "ignore"' % c for c in ignored)
    return """{
  "board": {
    "design_settings": {
      "rule_severities": {
%s
      }
    }
  },
  "meta": {
    "filename": "%s.kicad_pro",
    "version": 3
  }
}
""" % (sev, name)


# ---- track angle / segment length ---------------------------------------------------------

def angle_board(pads=False):
    b = header(2)
    # 90 degree corner, 135 degree bend, straight continuation, sharp turn
    b += seg(10, 10, 20, 10) + seg(20, 10, 20, 20)
    b += seg(30, 10, 40, 10) + seg(40, 10, 47, 17)
    b += seg(50, 10, 60, 10) + seg(60, 10, 70, 10)
    b += seg(10, 30, 20, 30) + seg(20, 30, 12, 34)
    # T junctions (end on the middle of the other segment), square and slanted
    b += seg(30, 30, 40, 30) + seg(35, 30, 35, 40)
    b += seg(50, 30, 60, 30) + seg(55, 30, 58, 36)
    b += seg(70, 30, 80, 30) + seg(75, 30, 71, 33)
    # three segments meeting in one node, reversed directions, a joint on a via
    b += seg(10, 50, 20, 50) + seg(20, 60, 20, 50) + seg(27, 43, 20, 50)
    b += seg(40, 50, 30, 50) + seg(40, 50, 40.1, 55)
    b += seg(50, 50, 60, 50) + seg(60, 50, 66, 52) + via(60, 50, "F.Cu", "B.Cu", 0.3, 0.6,
                                                         "through", None, "N1")
    # an arc in a chain (arcs are not tested), crossing segments, another layer
    b += seg(10, 70, 20, 70) + arc(20, 70, 23, 71, 25, 74) + seg(25, 74, 25, 80)
    b += seg(40, 70, 50, 75, "N2") + seg(40, 75, 50, 70, "N2")
    b += seg(60, 70, 70, 70, layer="B.Cu") + seg(70, 70, 75, 72, layer="B.Cu")
    # nearly straight and nearly square joints (rounding of the angle, precise formatting)
    b += seg(10, 90, 20, 90) + seg(20, 90, 30, 90.001)
    b += seg(40, 90, 50, 90) + seg(50, 90, 50.0001, 100)

    if pads:
        # a joint on a pad is not checked; one next to it is
        b += seg(10, 110, 20, 110) + seg(20, 110, 20, 120) + smd_pad(20, 110)
        b += seg(30, 110, 40, 110) + seg(40, 110, 40, 120) + smd_pad(42, 110)

    return b + ")\n"


def length_board():
    b = header(2)
    for i, ln in enumerate((0.5, 1.9999, 2, 2.0001, 5, 7.9999, 8, 8.00001, 9, 25)):
        b += seg(10, 10 + 3 * i, 10 + ln, 10 + 3 * i)
    b += seg(40, 10, 43, 14) + seg(40, 20, 40.0003, 20.0004)
    b += seg(60, 10, 70, 10, layer="B.Cu") + seg(60, 20, 61, 20, layer="B.Cu")
    # arcs: short, long, half circle
    b += arc(40, 40, 41, 40.3, 42, 40) + arc(40, 50, 45, 53, 50, 50)
    b += arc(60, 40, 61, 41, 62, 40) + arc(60, 60, 64, 64, 68, 60)
    b += via(80, 80, "F.Cu", "B.Cu", 0.3, 0.6, "through", None, "N1")
    return b + ")\n"


# ---- via stack (test_drc_via_stack.cpp) ---------------------------------------------------

def uniform4(prepreg=True):
    """`setUniformStackup`: 0.035 mm copper and 0.1 mm prepreg on 4 layers."""
    return stackup(4, {}, [("prepreg", [0.1])] * 3)


STACK6 = [("prepreg", [0.1]), ("core", [0.3]), ("prepreg", [0.2]), ("core", [0.3]),
          ("prepreg", [0.1])]


def case_good_stack():
    # GoodStackNoViolations: a regenerated F.Cu - In2.Cu stack over 0.1 mm prepreg
    u1, u2 = uid(), uid()
    b = header(4, stackup(4, {}, [("prepreg", [0.1])] * 3))
    b += via(10, 10, "F.Cu", "In1.Cu", 0.15, 0.3, u=u1)
    b += via(10, 10, "In1.Cu", "In2.Cu", 0.15, 0.3, u=u2)
    b += via_stack("F.Cu", "In2.Cu", [u1, u2], 10, 10)
    return b + ")\n", rule("hole_aspect_ratio (max 1.0)"), None


def case_duplicate_hop():
    # DuplicateHopReported
    u1, u2, u3 = uid(), uid(), uid()
    b = header(4)
    b += via(10, 10, "F.Cu", "In1.Cu", u=u1) + via(10, 10, "In1.Cu", "In2.Cu", u=u2)
    b += via(10, 10, "F.Cu", "In1.Cu", u=u3)
    b += via_stack("F.Cu", "In2.Cu", [u1, u2, u3], 10, 10)
    return b + ")\n", None, None


def case_stack_variants():
    # a stack whose span names a layer the board does not have, one with a gap, one whose
    # members are not microvias (not checked), one tiling B.Cu - In2.Cu from the back
    b = header(4)
    u = [uid() for _ in range(6)]
    b += via(30, 10, "F.Cu", "In1.Cu", u=u[0])
    b += via_stack("F.Cu", "In3.Cu", [u[0]], 30, 10)
    b += via(40, 10, "F.Cu", "In1.Cu", u=u[1]) + via(40, 10, "In2.Cu", "B.Cu", u=u[2])
    b += via_stack("F.Cu", "B.Cu", [u[1], u[2]], 40, 10)
    b += via(50, 10, "F.Cu", "In1.Cu", kind="blind", u=u[3])
    b += via_stack("F.Cu", "In1.Cu", [u[3]], 50, 10)
    b += via(60, 10, "B.Cu", "In2.Cu", u=u[4]) + via(60, 10, "In2.Cu", "In1.Cu", u=u[5])
    b += via_stack("B.Cu", "In1.Cu", [u[4], u[5]], 60, 10)
    return b + ")\n", rule("microvia_stack_depth (max 1)"), None


def micro6(stack=None):
    b = header(6, stack)
    b += via(10, 10, "F.Cu", "In1.Cu", 0.15, 0.3)
    return b + ")\n"


def case_too_deep():
    # MicroviaTooDeepForItsDiameterIsReported
    return micro6(), rule("hole_aspect_ratio (max 1.0)"), None


def case_no_limit():
    # AspectRatioIsSilentWithoutALimit (first run)
    return micro6(), None, None


def case_others_ignored():
    # AspectRatioSurvivesTheOtherChecksBeingIgnored
    return micro6(), rule("hole_aspect_ratio (max 1.0)"), [
        "malformed_microvia_stack_span", "microvia_stack_not_filled", "microvia_stack_depth"]


def case_default_stackup():
    # AspectRatioFallsBackToTheDefaultStackup
    b = header(4) + via(10, 10, "F.Cu", "In1.Cu", 0.15, 0.3)
    return b + ")\n", rule("hole_aspect_ratio (max 0.5)"), None


def case_ignored_depth():
    # IgnoredDepthRuleIsSilent
    u1, u2 = uid(), uid()
    b = header(4)
    b += via(10, 10, "F.Cu", "In1.Cu", u=u1) + via(10, 10, "In1.Cu", "In2.Cu", u=u2)
    b += via_stack("F.Cu", "In2.Cu", [u1, u2], 10, 10)
    return b + ")\n", rule("microvia_stack_depth (max 1)", severity="ignore"), None


def case_ignored_aspect():
    # IgnoredAspectRuleIsSilent (the modified stackup is not m_HasStackup: default stackup)
    b = header(4) + via(10, 10, "F.Cu", "In1.Cu", 0.15, 0.3)
    return b + ")\n", rule("hole_aspect_ratio (max 0.1)", severity="ignore"), None


def case_sublayers():
    # AspectRatioCountsEveryDielectricSublayer
    st = stackup(6, {}, [("prepreg", [0.1, 0.1])] + STACK6[1:])
    return micro6(st), rule("hole_aspect_ratio (max 1.2)"), None


def case_back_foil():
    # AspectRatioUsesTheFoilTheLaserEnters
    cu = {"F.Cu": 0.035, "B.Cu": 0.035, "In1.Cu": 0.0175, "In2.Cu": 0.0175, "In3.Cu": 0.0175,
          "In4.Cu": 0.0175}
    b = header(6, stackup(6, cu, STACK6))
    b += via(10, 10, "In4.Cu", "B.Cu", 0.1, 0.3)
    return b + ")\n", rule("hole_aspect_ratio (max 1.25)"), None


def case_core():
    # MicroviaThroughCoreIsReported: one over the first core (In1 - In2), one over the first
    # prepreg (F - In1); no aspect limit
    b = header(6, stackup(6, {}, STACK6))
    b += via(10, 10, "In1.Cu", "In2.Cu", 0.15, 0.3) + via(30, 10, "F.Cu", "In1.Cu", 0.15, 0.3)
    # and a microvia through two dielectrics incl. a disabled-free core, and a through via
    b += via(50, 10, "F.Cu", "In2.Cu", 0.15, 0.3) + via(70, 10, "F.Cu", "B.Cu", 0.3, 0.6,
                                                        "through")
    return b + ")\n", None, None


def case_ignored_aspect_core():
    # AnIgnoredAspectRatioDoesNotStopTheCoreCheck
    b = header(6, stackup(6, {}, STACK6))
    b += via(10, 10, "In1.Cu", "In2.Cu", 0.15, 0.3) + via(30, 10, "In1.Cu", "In2.Cu", 0.15, 0.3)
    return b + ")\n", rule("hole_aspect_ratio (max 0.1)"), ["hole_aspect_ratio"]


def case_through():
    # ThroughViaTooDeepForItsDrillIsReported
    b = header(4, uniform4())
    b += via(10, 10, "F.Cu", "B.Cu", 0.05, 0.25, "through", None)
    b += via(20, 10, "F.Cu", "B.Cu", 0.1, 0.3, "through", None)
    return b + ")\n", rule("hole_aspect_ratio (max 6.0)"), None


def case_blind():
    # BlindViaMeasuresItsSpanNotTheBoard
    b = header(4, uniform4())
    b += via(10, 10, "F.Cu", "In1.Cu", 0.05, 0.25, "blind", None)
    return b + ")\n", rule("hole_aspect_ratio (max 6.0)"), None


def case_buried():
    # BuriedViaCountsTheFoilItDrillsOutOf
    b = header(4, uniform4())
    b += via(10, 10, "In1.Cu", "In2.Cu", 0.05, 0.25, "buried", None)
    return b + ")\n", rule("hole_aspect_ratio (max 3.0)"), None


def case_pads():
    # PlatedPadHoleUsesItsNarrowDimension
    b = header(4, uniform4())
    b += pth_pad(10, 10, 0.2, 0.05) + pth_pad(20, 10, 0.1, 0.1)
    return b + ")\n", rule("hole_aspect_ratio (max 6.0)"), None


def case_npth():
    # UnplatedHoleIsNotChecked
    b = header(4, uniform4())
    b += pth_pad(10, 10, 0.05, 0.05, npth=True)
    return b + ")\n", rule("hole_aspect_ratio (max 6.0)"), None


def case_via_type_condition():
    # AViaTypeConditionScopesTheLimit (needs rule conditions: DRC-21)
    b = header(4, uniform4())
    b += via(10, 10, "F.Cu", "In1.Cu", 0.1, 0.3)
    b += via(20, 10, "F.Cu", "B.Cu", 0.1, 0.3, "through", None)
    return b + ")\n", rule("hole_aspect_ratio (max 1.0)", "A.Via_Type == 'Micro'"), None


def case_ratio_format():
    # ratios and limits that exercise the %.2f formatting (ties, three-decimal limits)
    b = header(4, uniform4())
    for i, drill in enumerate((0.04, 0.0625, 0.08, 0.088, 0.1, 0.11)):
        b += via(10 + 5 * i, 10, "F.Cu", "B.Cu", drill, drill + 0.2, "through", None)
    return b + ")\n", rule("hole_aspect_ratio (max 4.125)"), None


SYNTH = [
    ("angles", lambda: (angle_board(), rule("track_angle (min 100) (max 170)"), None)),
    ("angles_min", lambda: (angle_board(), rule("track_angle (min 135deg)"), None)),
    ("angles_pads", lambda: (angle_board(True), rule("track_angle (min 100)"), None)),
    ("segment_length", lambda: (length_board(),
                                rule("track_segment_length (min 2mm) (max 8mm)"), None)),
    ("vs_good_stack", case_good_stack),
    ("vs_duplicate_hop", case_duplicate_hop),
    ("vs_stack_variants", case_stack_variants),
    ("vs_too_deep", case_too_deep),
    ("vs_no_limit", case_no_limit),
    ("vs_others_ignored", case_others_ignored),
    ("vs_default_stackup", case_default_stackup),
    ("vs_ignored_depth", case_ignored_depth),
    ("vs_ignored_aspect", case_ignored_aspect),
    ("vs_sublayers", case_sublayers),
    ("vs_back_foil", case_back_foil),
    ("vs_core", case_core),
    ("vs_ignored_aspect_core", case_ignored_aspect_core),
    ("vs_through", case_through),
    ("vs_blind", case_blind),
    ("vs_buried", case_buried),
    ("vs_pads", case_pads),
    ("vs_npth", case_npth),
    ("vs_via_type_condition", case_via_type_condition),
    ("vs_ratio_format", case_ratio_format),
]

# The board-file cases of test_drc_via_stack.cpp that set a rule: copied from
# qa/data/pcbnew/drc_via_stack/<board> with the rule file written next to it.
QA = [
    ("qa_StackTooDeepReported", "StackTooDeepReported", rule("microvia_stack_depth (max 1)")),
    ("qa_LooseStackedMicroviasReportDepth", "LooseStackedMicroviasReportDepth",
     rule("microvia_stack_depth (max 2)")),
    ("qa_CoaxialButSeparatedMicroviasAreNotOneStack", "CoaxialButSeparatedMicroviasAreNotOneStack",
     rule("microvia_stack_depth (max 1)")),
    ("qa_MicroviaStackDepthRuleIsHonoured_2", "MicroviaStackDepthRuleIsHonoured",
     rule("microvia_stack_depth (max 2)")),
    ("qa_MicroviaStackDepthRuleIsHonoured_4", "MicroviaStackDepthRuleIsHonoured",
     rule("microvia_stack_depth (max 4)")),
    ("qa_MicroviaStackDepthRuleIsHonoured_cond", "MicroviaStackDepthRuleIsHonoured",
     rule("microvia_stack_depth (max 2)", "A.isStackedVia()")),
]


def run_reference(cli, src_dir, name):
    with tempfile.TemporaryDirectory(prefix="drc28-") as tmp:
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
            if os.path.exists(os.path.join(src_dir, name + ext)):
                shutil.copy(os.path.join(src_dir, name + ext), os.path.join(tmp, name + ext))

        out = os.path.join(tmp, "out.json")
        env = dict(os.environ)
        env.setdefault("KICAD_ORACLE_HOME", os.path.join(tmp, "home"))
        args = [cli, "pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]
        subprocess.run(args + ["-o", out, os.path.join(tmp, name + ".kicad_pcb")],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env, cwd=tmp,
                       timeout=600)

        with open(out) as f:
            return json.load(f)


def write_golden(name, report):
    lines = []

    for v in report.get("violations", []):
        if v["type"] not in TYPES:
            continue

        fields = [v["severity"], v["type"], v["description"]]

        for it in v.get("items", []):
            fields += [it["uuid"], it["description"], repr(it["pos"]["x"]),
                       repr(it["pos"]["y"])]

        lines.append("\t".join(f.replace("\t", " ").replace("\n", " ") for f in fields))

    with open(os.path.join(GOLDEN, name + ".txt"), "w") as f:
        f.write("".join(line + "\n" for line in lines))

    print("%s: %d entries" % (name, len(lines)))


def write(name, board, dru, ignored):
    for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
        if os.path.exists(os.path.join(DATA, name + ext)):
            os.remove(os.path.join(DATA, name + ext))

    with open(os.path.join(DATA, name + ".kicad_pcb"), "w") as f:
        f.write(board)

    if dru:
        with open(os.path.join(DATA, name + ".kicad_dru"), "w") as f:
            f.write(dru)

    if ignored:
        with open(os.path.join(DATA, name + ".kicad_pro"), "w") as f:
            f.write(pro(name, ignored))


def main():
    cli = "/home/user/kicad-oracle/bin/kicad-cli"

    if "--cli" in sys.argv:
        cli = sys.argv[sys.argv.index("--cli") + 1]

    os.makedirs(DATA, exist_ok=True)
    os.makedirs(GOLDEN, exist_ok=True)
    qa = os.path.join(HERE, "..", "..", "..", "..", "..", "qa", "data", "pcbnew", "drc_via_stack")

    for name, make in SYNTH:
        board, dru, ignored = make()
        write(name, board, dru, ignored)
        write_golden(name, run_reference(cli, DATA, name))

    for name, src, dru in QA:
        with open(os.path.join(qa, src + ".kicad_pcb")) as f:
            write(name, f.read(), dru, None)

        write_golden(name, run_reference(cli, DATA, name))


if __name__ == "__main__":
    main()
