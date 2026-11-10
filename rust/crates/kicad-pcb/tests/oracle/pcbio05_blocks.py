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
# PCBIO-05 golden helper (used by pcbio05_golden.sh).
#
#   pcbio05_blocks.py extract BOARD   a texts-only board on stdout: the header lists of BOARD
#                                     (version .. setup) and its top-level gr_text / gr_text_box,
#                                     with every (render_cache ..) removed (the C++ drops loaded
#                                     caches anyway and regenerates them when saving)
#   pcbio05_blocks.py strip BOARD     BOARD on stdout with the (polygon ..) lists inside every
#                                     (render_cache ..) removed: the oracle regenerates the glyphs
#                                     from the host fonts with the outline font engine, which the
#                                     port does not have (kicad-font D1); the rest of the cache
#                                     header (text, angle) is still compared; the
#                                     emptied list is closed on its line, as the formatter
#                                     prints a list without children
import re
import sys

HEADER = {"version", "generator", "generator_version", "host", "general", "paper", "page",
          "title_block", "layers", "setup"}
TEXTS = {"gr_text", "gr_text_box"}


def spans(t):
    """(start, end, depth, keyword) of every list of t."""
    out, stack, i = [], [], 0
    while i < len(t):
        c = t[i]
        if c == '"':
            i += 1
            while t[i] != '"':
                i += 2 if t[i] == '\\' else 1
        elif c == '(':
            j = i + 1
            while t[j] in ' \t\r\n':
                j += 1
            k = j
            while t[k] not in ' \t\r\n()"':
                k += 1
            stack.append((i, t[j:k]))
        elif c == ')':
            s, kw = stack.pop()
            out.append((s, i, len(stack), kw))
        i += 1
    return out


def remove(t, cut):
    """t without the byte spans in cut (non-overlapping), plus the blanks before each."""
    res, last = [], 0
    for s, e in sorted(cut):
        b = s
        while b > last and t[b - 1] in ' \t\r\n':
            b -= 1
        res.append(t[last:b])
        last = e + 1
    res.append(t[last:])
    return ''.join(res)


def extract(t):
    sp = spans(t)
    parts = [t[s:e + 1] for s, e, d, kw in sorted(sp) if d == 1 and kw in HEADER | TEXTS]
    body = "(kicad_pcb\n\t" + "\n\t".join(parts) + "\n)\n"
    cut = [(s, e) for s, e, d, kw in spans(body) if kw == "render_cache"]
    return remove(body, [c for c in cut if not any(o != c and o[0] <= c[0] and c[1] <= o[1]
                                                  for o in cut)])


def strip(t):
    sp = spans(t)
    caches = [(s, e) for s, e, d, kw in sp if kw == "render_cache"]
    cut = [(s, e) for s, e, d, kw in sp if kw == "polygon"
           and any(cs < s and e < ce for cs, ce in caches)]
    cut = [c for c in cut if not any(o != c and o[0] <= c[0] and c[1] <= o[1] for o in cut)]
    return re.sub(r'(\(render_cache "(?:[^"\\]|\\.)*" [^\s()]+)\s+\)', r'\1)', remove(t, cut))


mode, path = sys.argv[1], sys.argv[2]
text = open(path, encoding='utf-8').read()
sys.stdout.write(extract(text) if mode == "extract" else strip(text))
