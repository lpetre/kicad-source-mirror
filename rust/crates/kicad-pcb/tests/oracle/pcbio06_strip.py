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

"""Reduce a .kicad_pcb to its footprints for the PCBIO-06 oracle (pcbio06_golden.sh).

Usage: pcbio06_strip.py IN OUT [--max-footprints N]

Keeps the board header and settings (version, generator, general, paper / page, title block,
layers, setup, nets, legacy net classes, properties, variants, embedded fonts / files) and the
footprints. Inside each footprint every token is kept except: pads (PCBIO-07), groups and
constraints (their members would be gone), every zone but the first, and every drawing child
(fp_text user, fp_text_box, fp_line .. fp_ellipse_arc, image, barcode, dimension, table) but
the first: `FOOTPRINT::cmp_drawings` / `cmp_zones` (package M20) are not needed to order a
single child. `fp_text reference` / `fp_text value` (legacy fields) are kept.
"""

import sys

TOP_KEEP = {
    b'version', b'generator', b'generator_version', b'host', b'general', b'paper', b'page',
    b'title_block', b'layers', b'setup', b'net', b'net_class', b'property', b'variants',
    b'embedded_fonts', b'embedded_files', b'footprint', b'module',
}
DRAWINGS = {
    b'fp_text', b'fp_text_box', b'fp_line', b'fp_rect', b'fp_circle', b'fp_arc', b'fp_poly',
    b'fp_curve', b'fp_ellipse', b'fp_ellipse_arc', b'image', b'barcode', b'dimension', b'table',
}
FP_DROP = {b'pad', b'group', b'constraint'}


def lists(data, start, end):
    """The direct child lists of data[start:end] as (keyword, open, close+1) spans."""
    out = []
    i = start
    depth = 0
    cur = None
    while i < end:
        c = data[i:i + 1]
        if c == b'"':
            i += 1
            while i < end and data[i:i + 1] != b'"':
                if data[i:i + 1] == b'\\':
                    i += 1
                i += 1
        elif c == b'(':
            if depth == 0:
                cur = i
            depth += 1
        elif c == b')':
            depth -= 1
            if depth == 0:
                j = cur + 1
                k = j
                while k < end and data[k:k + 1] not in (b' ', b'\t', b'\n', b'\r', b'(', b')'):
                    k += 1
                out.append((data[j:k], cur, i + 1))
        i += 1
    return out


def second_word(data, s, e):
    """The word after the keyword of the list data[s:e] (fp_text type)."""
    parts = data[s + 1:e].split(None, 2)
    return parts[1] if len(parts) > 1 else b''


def strip(data, max_footprints):
    root = lists(data, 0, len(data))[0]
    _, rs, re_ = root
    # skip "(kicad_pcb"
    removes = []
    footprints = 0
    for kw, s, e in lists(data, rs + 1, re_ - 1):
        if kw not in TOP_KEEP:
            removes.append((s, e))
            continue
        if kw not in (b'footprint', b'module'):
            continue
        footprints += 1
        if max_footprints and footprints > max_footprints:
            removes.append((s, e))
            continue
        seen_drawing = False
        seen_zone = False
        for ckw, cs, ce in lists(data, s + 1, e - 1):
            if ckw in FP_DROP:
                removes.append((cs, ce))
            elif ckw == b'zone':
                if seen_zone:
                    removes.append((cs, ce))
                seen_zone = True
            elif ckw in DRAWINGS:
                if ckw == b'fp_text' and second_word(data, cs, ce) in (b'reference', b'value'):
                    continue
                if seen_drawing:
                    removes.append((cs, ce))
                seen_drawing = True
    out = bytearray()
    last = 0
    for s, e in sorted(removes):
        out += data[last:s]
        last = e
    out += data[last:]
    # drop the lines left blank by the removals
    lines = [l for l in bytes(out).split(b'\n') if l.strip() != b'']
    return b'\n'.join(lines) + b'\n'


def main():
    args = sys.argv[1:]
    max_fp = 0
    if '--max-footprints' in args:
        i = args.index('--max-footprints')
        max_fp = int(args[i + 1])
        del args[i:i + 2]
    src, dst = args
    data = open(src, 'rb').read()
    open(dst, 'wb').write(strip(data, max_fp))


if __name__ == '__main__':
    main()
