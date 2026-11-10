#!/usr/bin/env python3
# Regenerates m12_dimensions.txt (package M12, PCB_DIMENSION_BASE and the PCB_DIM_* classes).
#
# Runs the reference `pcb-dump board --sections item,bbox,dim,text` on copies of the corpus
# boards holding dimensions of every type (BOARDS) and on m12_dimensions.kicad_pcb (extra
# variants: inward arrows, inline / manual text, paren suffix, zero suppression, the V_VV
# precisions, a mirrored bold text on B.SilkS, a ${LAYER} suffix, a circle-framed leader, and an
# aligned and a radial dimension in a footprint placed with
# `(transform (translate 10 20) (rotate 30) (scale 1.5 0.75))` = test_support's
# rotated_scaled_transform()). For every dimension it writes
#   in|UUID|field=value      the stored members the parser would set (inputs of the Rust test)
#   SECTION|UUID|field=value the reference values after the loader's Update()
# Long values (tpoly.*, text_polys.*, text_shape, effective_shape) are replaced by the FNV-1a
# hash of their text (`#<16 hex>`, as `pcb-dump --hash`).
# Run from <kicad>/rust; needs the oracle (docs/ORACLE.md).
import os
import subprocess
import sys
import tempfile

KICAD = os.path.abspath('..')
HERE = 'crates/kicad-pcb/tests/golden'
OUT = os.path.join(HERE, 'm12_dimensions.txt')
DUMP = os.environ.get('PCB_DUMP', '/home/user/kicad-oracle/bin/pcb-dump')

BOARDS = [
    'qa/data/pcbnew/issue20249/dimension.kicad_pcb',
    'qa/data/pcbnew/issue24853/issue24853.kicad_pcb',
    'qa/data/pcbnew/api_kitchen_sink.kicad_pcb',
    'qa/data/pcbnew/issue24192/issue24192.kicad_pcb',
    'demos/constraints/constraints.kicad_pcb',
    os.path.join('rust', HERE, 'm12_dimensions.kicad_pcb'),
]

DIM_TYPES = {'PCB_DIM_ALIGNED_T', 'PCB_DIM_ORTHOGONAL_T', 'PCB_DIM_RADIAL_T',
             'PCB_DIM_LEADER_T', 'PCB_DIM_CENTER_T'}

IN_DIM = ['lib_start', 'lib_end', 'override_text_enabled', 'override_text', 'prefix', 'suffix',
          'arrow_direction', 'units', 'units_mode', 'units_format', 'precision',
          'suppress_zeroes', 'keep_text_aligned', 'text_position_mode', 'arrow_length',
          'extension_offset', 'line_thickness', 'height', 'extension_height', 'orientation',
          'leader_length', 'text_border']
IN_TEXT = ['text_pos', 'text_size', 'text_thickness', 'text_angle', 'italic', 'bold', 'visible',
           'mirrored', 'multiline', 'h_justify', 'v_justify', 'keep_upright', 'line_spacing']
IN_ITEM = ['type', 'layer', 'parent']
OUT_ITEM = ['position']
HASHED = ('tpoly.', 'text_polys.', 'text_shape', 'effective_shape')

# Text size / thickness of the footprint dimensions as the file holds them (board frame, set
# with SetTextSize / SetTextThickness by the parser; the dump shows them after the lib-frame
# round trip).
FP_TEXT = {'text_size': '1000000,1000000', 'text_thickness': '150000'}


def fnv1a64(s):
    h = 0xcbf29ce484222325
    for c in s.encode():
        h ^= c
        h = (h * 0x100000001b3) & 0xffffffffffffffff
    return '#%016x' % h


def dump(path, home):
    env = dict(os.environ, KICAD_ORACLE_HOME=home)
    r = subprocess.run([DUMP, 'board', '--sections', 'item,bbox,dim,text', path],
                       capture_output=True, text=True, env=env, check=True)
    rows = []
    for line in r.stdout.splitlines():
        parts = line.split('|', 2)
        if len(parts) != 3 or '=' not in parts[2]:
            continue
        field, value = parts[2].split('=', 1)
        rows.append((parts[0], parts[1], field, value))
    return rows


def main():
    out = []
    with tempfile.TemporaryDirectory() as tmp:
        home = os.path.join(tmp, 'home')
        os.mkdir(home)
        for rel in BOARDS:
            src = os.path.join(KICAD, rel)
            dst = os.path.join(tmp, os.path.basename(rel))
            with open(src, 'rb') as f, open(dst, 'wb') as g:
                g.write(f.read())
            rows = dump(dst, home)
            dims = [k for (s, k, f, v) in rows if s == 'item' and f == 'type' and v in DIM_TYPES]
            vals = {}
            for s, k, f, v in rows:
                vals[(s, k, f)] = v
            out.append('# ' + rel)
            for key in dims:
                in_fp = vals[('item', key, 'parent')] != 'board'
                for f in IN_ITEM:
                    out.append('in|%s|%s=%s' % (key, f, vals[('item', key, f)]))
                for f in IN_DIM:
                    if ('dim', key, f) in vals:
                        out.append('in|%s|%s=%s' % (key, f, vals[('dim', key, f)]))
                for f in IN_TEXT:
                    v = vals[('text', key, f)]
                    if in_fp and f in FP_TEXT:
                        v = FP_TEXT[f]
                    out.append('in|%s|%s=%s' % (key, f, v))
                for s, k, f, v in rows:
                    if k != key or (s == 'item' and f not in OUT_ITEM):
                        continue
                    if f.startswith(HASHED):
                        v = fnv1a64(v)
                    out.append('%s|%s|%s=%s' % (s, k, f, v))
    with open(OUT, 'w') as f:
        f.write('\n'.join(out) + '\n')
    print('wrote %s (%d lines)' % (OUT, len(out)))


if __name__ == '__main__':
    sys.exit(main())
