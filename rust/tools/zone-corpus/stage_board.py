# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
"""Make a DebugZoneFiller variant of a 2-layer board: 22 copper layers, every zone on F.Cu also
on In1..In19 (whose fills then hold the stage dumps of the F.Cu fill)."""
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
t = open(src).read()

if '"In1.Cu"' in t or '(2 "B.Cu"' not in t:
    sys.exit(2)

inner = ''.join(f'\t\t({2 * k + 2} "In{k}.Cu" signal)\n' for k in range(1, 21))
t = re.sub(r'(\n\s*)\(2 "B\.Cu" (signal|power|mixed|jumper)\)', lambda m: '\n' + inner.rstrip('\n') + m.group(0), t, count=1)
zl = ' '.join(f'"In{k}.Cu"' for k in range(1, 20))

out = []
i = 0
while True:
    j = t.find('\n\t(zone\n', i)
    if j < 0:
        out.append(t[i:])
        break
    out.append(t[i:j])
    # zone header up to the polygon
    k = t.find('(polygon', j)
    head = t[j:k]
    head = head.replace('(layer "F.Cu")', '(layers "F.Cu" ' + zl + ')')
    head = re.sub(r'\(layers "F\.Cu"([^)]*)\)', lambda m: '(layers "F.Cu"' + m.group(1) + ('' if 'In1.Cu' in m.group(1) else ' ' + zl) + ')', head, count=1)
    out.append(head)
    i = k
open(dst, 'w').write(''.join(out))
