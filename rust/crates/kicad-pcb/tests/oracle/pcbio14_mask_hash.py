#!/usr/bin/env python3
"""FNV-1a 64 of a written .kicad_mod after masking every UUID that does not occur in the input
(replaced by the text UUID). Same scanner as `mask_fresh_uuids` in tests/pcbio14_fp_lib.rs."""
import re
import sys

PAT = re.compile(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}')
inp = open(sys.argv[1], 'rb').read().decode('utf-8', 'replace')
known = set(PAT.findall(inp))
out = open(sys.argv[2], 'rb').read().decode('utf-8')
masked = PAT.sub(lambda m: m.group(0) if m.group(0) in known else 'UUID', out)
h = 0xcbf29ce484222325
for b in masked.encode('utf-8'):
    h ^= b
    h = (h * 0x100000001b3) & 0xffffffffffffffff
print('%016x' % h)
