#!/bin/bash
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
# Build the PCBIO-12 parse + write driver (pcbio12_resave.cpp) against the reference build. It
# reuses, read-only, the compile flags, the QA helper objects and the link line of
# rust/tools/pcb-dump (run rust/tools/pcb-dump/build.sh once first); only the driver itself is
# compiled. Machine-wide C++ build lock, one job (PORTING.md section 7).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dump_obj=${DUMP_OBJ_DIR:-/home/user/kicad-oracle/build-pcb-dump}
obj=${OBJ_DIR:-/home/user/kicad-oracle/build-pcbio12}
out=${OUT:-/home/user/kicad-oracle/bin/pcbio12-resave}
mkdir -p "$obj"

eval "CFLAGS=( $(cat "$dump_obj/cflags.txt") )"
kicad=$(cd "$here/../../../../.." && pwd)
CFLAGS+=( -I"$kicad/qa/qa_utils/include" -I"$kicad/qa/pcbnew_utils/include" -I"$kicad/qa/mocks/include"
          -Wno-shadow -Wno-unused-variable )

flock /home/user/kicad-oracle/cxx-build.lock nice -n 10 \
    c++ "${CFLAGS[@]}" -c "$here/pcbio12_resave.cpp" -o "$obj/pcbio12_resave.o"

# The pcb-dump link line with its own objects replaced by the driver (the QA helper objects and
# the header-only Boost.Test implementation are kept).
python3 - "$dump_obj/link.sh" "$obj/pcbio12_resave.o" "$out" > "$obj/link.sh" <<'PY'
import shlex, sys
link, main, out = sys.argv[1:]
cmd = open(link).read().strip()
pre, rest = cmd.split(" && ", 1)
args = shlex.split(rest)
res, skip = [], False
for a in args:
    if skip:
        skip = False
        continue
    if a == "-o":
        res += ["-o", out, main]
        skip = True
        continue
    if "build-pcb-dump/rust_tools_pcb-dump_" in a and not a.endswith("boost_test_impl.cpp.o"):
        continue
    res.append(a)
print(pre + " && " + " ".join(shlex.quote(a) for a in res))
PY
flock /home/user/kicad-oracle/cxx-build.lock bash "$obj/link.sh"
echo "built $out"
