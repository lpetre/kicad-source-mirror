#!/bin/bash
# Package SET-3 differential oracle: builds tests/oracle/net_settings_json_oracle.cpp against the
# reference build's libkicommon.so (real NET_SETTINGS JSON parameters and migrations), runs it on
# the hand-written documents in tests/data/net_settings_json (full output) and on every .kicad_pro
# under qa/ and demos/ (compact output) and writes tests/golden/net_settings_json.txt, replayed by
# the Rust test tests/net_settings_json.rs (`oracle_documents`).
#
# Usage: tests/oracle/net_settings_json_golden.sh   (run from anywhere)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../../../.." && pwd)
build=${ORACLE_BUILD:-/home/user/kicad-oracle/build}
obj=${OBJ_DIR:-/home/user/kicad-oracle/build-net-settings-oracle}
mkdir -p "$obj"

# Compile flags of common/project/net_settings.cpp (kicommon), without the PCH and outputs.
python3 - "$build" "$obj/cflags.txt" <<'PY'
import json, shlex, sys
build, out = sys.argv[1], sys.argv[2]
for e in json.load(open(build + "/compile_commands.json")):
    if e["file"].endswith("common/project/net_settings.cpp"):
        args = shlex.split(e["command"])[1:]
        keep, skip = [], False
        for a in args:
            if skip: skip = False; continue
            if a in ("-o", "-c", "-include", "-x"): skip = True; continue
            if a.endswith(".cpp") or a.startswith("-Winvalid-pch") or a.startswith("-fvisibility"):
                continue
            if a == "-Dkicommon_EXPORTS": continue
            keep.append(a)
        open(out, "w").write(" ".join(shlex.quote(a) for a in keep))
        break
else:
    sys.exit("net_settings.cpp not found in compile_commands.json")
PY
eval "CFLAGS=( $(cat "$obj/cflags.txt") )"
nice -n 15 c++ "${CFLAGS[@]}" -Wno-shadow "$here/oracle/net_settings_json_oracle.cpp" \
    -o "$obj/net_settings_json_oracle" \
    -L"$build/common" -lkicommon -Wl,-rpath,"$build/common" $(wx-config --libs base,core)

list="$obj/net_settings_json_list.txt"
( cd "$root"
  for f in $(ls rust/crates/kicad-pcb/tests/data/net_settings_json/*.kicad_pro | LC_ALL=C sort); do
      echo "F $f"; done
  find qa demos -name '*.kicad_pro' | LC_ALL=C sort | sed 's/^/C /' ) > "$list"
"$obj/net_settings_json_oracle" "$list" "$root" > "$here/golden/net_settings_json.txt"
wc -l "$here/golden/net_settings_json.txt"
