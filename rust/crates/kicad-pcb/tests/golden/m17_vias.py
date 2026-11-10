#!/usr/bin/env python3
# Regenerates m17_vias.txt (package M17, PCB_VIA): builds a 4-layer board from
# qa/data/pcbnew/drc_via_stack/AViaCarriedTwiceIsReportedOnce.kicad_pcb (its header and setup,
# with mask expansion 0.05 and front-only tenting / covering, back-only plugging, capping on)
# and the vias of VIAS below, runs the reference `pcb-dump board --sections item,bbox,track`
# on it and keeps the via fields the Rust test asserts. Polygons (`tpoly.*`) are replaced by
# the FNV-1a hash of their text. Keys are shortened to the last four UUID digits.
# Run from <kicad>/rust; needs the oracle (docs/ORACLE.md).
import os
import re
import subprocess
import sys
import tempfile

KICAD = os.path.abspath('..')
SRC = os.path.join(KICAD, 'qa/data/pcbnew/drc_via_stack/AViaCarriedTwiceIsReportedOnce.kicad_pcb')
OUT = 'crates/kicad-pcb/tests/golden/m17_vias.txt'
DUMP = os.environ.get('PCB_DUMP', '/home/user/kicad-oracle/bin/pcb-dump')

# (type, at, size, drill, layers, extra tokens); uuid ...b<index>
VIAS = [
    ('', '10 10', '0.6', '0.3', '"F.Cu" "B.Cu"', ''),
    ('', '12 10', '0.6', '0.3', '"F.Cu" "B.Cu"', '(tenting (front no) (back yes))'),
    ('blind', '14 10', '0.5', '0.2', '"F.Cu" "In1.Cu"', ''),
    ('buried', '16 10', '0.5', '0.2', '"In1.Cu" "In2.Cu"', ''),
    ('micro', '18 10', '0.3', '0.1', '"In2.Cu" "B.Cu"', ''),
    ('', '20 10', '0.6', '0.3', '"F.Cu" "B.Cu"',
     '(backdrill (size 0.5) (layers "B.Cu" "In2.Cu")) (tertiary_drill (size 0.45) (layers "F.Cu" "In1.Cu"))'),
    ('', '22 10', '0.6', '0.3', '"F.Cu" "B.Cu"',
     '(front_post_machining counterbore (size 0.8) (depth 0.2)) (back_post_machining countersink (size 0.9) (angle 90))'),
    ('', '24 10', '0.6', '0.3', '"F.Cu" "B.Cu"', '(remove_unused_layers yes) (keep_end_layers yes)'),
    ('', '26 10', '0.6', '0.3', '"F.Cu" "B.Cu"',
     '(remove_unused_layers yes) (keep_end_layers no) (zone_layer_connections "In1.Cu")'),
    ('blind', '28 10', '0.5', '0.2', '"F.Cu" "In2.Cu"', '(start_end_only yes)'),
    ('', '30 10', '0.6', '0.3', '"F.Cu" "B.Cu"',
     '(padstack (mode front_inner_back) (layer "Inner" (size 0.5)) (layer "B.Cu" (size 0.7)))'),
    ('blind', '32 10', '0.6', '0.25', '"In1.Cu" "B.Cu"',
     '(padstack (mode front_inner_back) (layer "Inner" (size 0.45)) (layer "B.Cu" (size 0.65)))'),
    ('', '34 10', '0.6', '0.3', '"F.Cu" "B.Cu"',
     '(padstack (mode custom) (layer "In1.Cu" (size 0.5)) (layer "In2.Cu" (size 0.55)) (layer "B.Cu" (size 0.7)))'),
    ('micro', '36 10', '0.3', '0.1', '"F.Cu" "In1.Cu"',
     '(tenting (front no) (back no)) (capping no) (covering (front no) (back yes)) (plugging (front yes) (back no)) (filling yes)'),
    ('blind', '38 10', '0.5', '0.2', '"B.Cu" "In1.Cu"', ''),
    ('', '40 10', '0.6', '0.3', '"F.Cu" "B.Cu"',
     '(backdrill (size 0.8) (layers "F.Cu" "In2.Cu")) (front_post_machining countersink (size 1.0) (angle 82))'),
    ('', '42.0005 -10.0003', '0.6011', '0.3', '"F.Cu" "B.Cu"', '(free yes)'),
]

KEEP = re.compile(r'^(item\|[^|]*\|(layer|layers|position|on_copper|has_hole|has_drilled_hole)='
                  r'|bbox\|'
                  r'|track\|[^|]*\|(?!padstack\.(layerset|layers|name|orientation|custom_shape_in_zone_mode'
                  r'|front_outer\.solder|back_outer\.solder|front_outer\.has_solder_paste'
                  r'|back_outer\.has_solder_paste|copper\.[A-Za-z0-9]+\.Cu\.(?!size=))))')


def fnv1a(s):
    h = 0xcbf29ce484222325
    for b in s.encode():
        h ^= b
        h = (h * 0x100000001b3) & 0xffffffffffffffff
    return '%016x' % h


def main():
    head = open(SRC).read().split('\t(gr_text')[0]
    head = head.replace('(pad_to_mask_clearance 0)', '(pad_to_mask_clearance 0.05)')
    head = head.replace('(tenting\n\t\t\t(front yes)\n\t\t\t(back yes)', '(tenting\n\t\t\t(front yes)\n\t\t\t(back no)')
    head = head.replace('(covering\n\t\t\t(front no)', '(covering\n\t\t\t(front yes)')
    head = head.replace('(plugging\n\t\t\t(front no)\n\t\t\t(back no)', '(plugging\n\t\t\t(front no)\n\t\t\t(back yes)')
    head = head.replace('(capping no)', '(capping yes)')
    body = [head]
    for i, (t, at, size, drill, layers, extra) in enumerate(VIAS, 1):
        body.append(f'\t(via {t}\n\t\t(at {at})\n\t\t(size {size})\n\t\t(drill {drill})\n'
                    f'\t\t(layers {layers})\n\t\t{extra}\n\t\t(net "")\n'
                    f'\t\t(uuid "00000000-0000-0000-0000-00000000b{i:03d}")\n\t)\n')
    body.append('\t(embedded_fonts no)\n)\n')

    with tempfile.TemporaryDirectory() as tmp:
        board = os.path.join(tmp, 'vias.kicad_pcb')
        open(board, 'w').write(''.join(body))
        env = dict(os.environ, KICAD_ORACLE_HOME=os.path.join(tmp, 'home'))
        out = subprocess.run([DUMP, 'board', '--sections', 'item,bbox,track', board],
                             env=env, check=True, capture_output=True, text=True).stdout

    lines = []
    for line in out.splitlines():
        if not KEEP.match(line):
            continue
        sec, key, rest = line.split('|', 2)
        key = key[-4:]
        field, value = rest.split('=', 1)
        if field.startswith('tpoly.'):
            value = fnv1a(value)
        lines.append(f'{sec}|{key}|{field}={value}')

    open(OUT, 'w').write('\n'.join(lines) + '\n')
    print(f'{OUT}: {len(lines)} lines', file=sys.stderr)


main()
