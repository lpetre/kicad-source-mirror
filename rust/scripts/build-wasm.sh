#!/bin/sh
# Build the WebAssembly module of the KiCad Rust port and its JavaScript bindings.
#
#   rust/scripts/build-wasm.sh            build + bindgen (+ wasm-opt when installed)
#   rust/scripts/build-wasm.sh --test     ... then run the node smoke test and the browser test
#
# Outputs (git-ignored):
#   rust/web/pkg-node/   wasm-bindgen --target nodejs (CommonJS, for node and the smoke test)
#   rust/web/pkg/        wasm-bindgen --target web    (ES module, for index.html / app.js)
#
# Toolchain (docs/THIRD_PARTY_EQUIVALENTS.md 4): target wasm32-unknown-unknown, wasm-bindgen-cli
# 0.2.130 (must equal the wasm-bindgen crate version pinned in crates/kicad-wasm/Cargo.toml),
# node 22 for the tests.
set -eu

RUST=$(cd "$(dirname "$0")/.." && pwd)
cd "$RUST"

NODE=${NODE:-/opt/node22/bin/node}
[ -x "$NODE" ] || NODE=node

echo "== cargo build -p kicad-wasm --target wasm32-unknown-unknown --profile wasm-release"
cargo build -p kicad-wasm --target wasm32-unknown-unknown --profile wasm-release

WASM="$RUST/target/wasm32-unknown-unknown/wasm-release/kicad_wasm.wasm"
[ -f "$WASM" ] || { echo "missing $WASM" >&2; exit 1; }

BINDGEN_VERSION=$(wasm-bindgen --version | awk '{print $2}')
CRATE_VERSION=$(sed -n 's/^wasm-bindgen = "=\(.*\)"/\1/p' "$RUST/crates/kicad-wasm/Cargo.toml")
if [ "$BINDGEN_VERSION" != "$CRATE_VERSION" ]; then
    echo "wasm-bindgen-cli $BINDGEN_VERSION does not match the crate version $CRATE_VERSION" >&2
    exit 1
fi

echo "== wasm-bindgen $BINDGEN_VERSION"
rm -rf "$RUST/web/pkg-node" "$RUST/web/pkg"
wasm-bindgen --target nodejs --out-dir "$RUST/web/pkg-node" "$WASM"
wasm-bindgen --target web --out-dir "$RUST/web/pkg" "$WASM"

size() { wc -c < "$1" | tr -d ' '; }
echo "== sizes"
echo "  cargo output        $(size "$WASM") bytes  $WASM"
echo "  after wasm-bindgen  $(size "$RUST/web/pkg/kicad_wasm_bg.wasm") bytes  web/pkg/kicad_wasm_bg.wasm"

if command -v wasm-opt >/dev/null 2>&1; then
    for d in pkg pkg-node; do
        wasm-opt -Os --enable-bulk-memory --enable-sign-ext --enable-mutable-globals \
            --enable-nontrapping-float-to-int --enable-reference-types --enable-multivalue \
            "$RUST/web/$d/kicad_wasm_bg.wasm" -o "$RUST/web/$d/kicad_wasm_bg.wasm"
    done
    echo "  after wasm-opt -Os  $(size "$RUST/web/pkg/kicad_wasm_bg.wasm") bytes"
else
    echo "  wasm-opt not installed: size optimisation skipped (optional, see docs/THIRD_PARTY_EQUIVALENTS.md 4)"
fi

if [ "${1:-}" = "--test" ]; then
    echo "== node smoke test"
    "$NODE" "$RUST/web/test/smoke.mjs"
    echo "== browser test"
    "$NODE" "$RUST/web/test/browser.mjs"
fi
