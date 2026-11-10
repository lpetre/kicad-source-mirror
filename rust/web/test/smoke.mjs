// This program source code file is part of KiCad, a free EDA CAD application.
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Node smoke test of the WebAssembly build (rust/web/pkg-node, made by scripts/build-wasm.sh):
// calls every implemented API function and compares the results with the native results
// recorded in crates/kicad-wasm/tests/golden/native_reference.json (native == wasm), and checks
// that the not-yet-ported calls fail with a NotYetPortedError.
//
//   /opt/node22/bin/node rust/web/test/smoke.mjs

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const here = dirname(fileURLToPath(import.meta.url));
const rust = join(here, '..', '..');
const kicad = join(rust, '..');
const require = createRequire(import.meta.url);
const k = require(join(rust, 'web', 'pkg-node', 'kicad_wasm.js'));
const ref = JSON.parse(readFileSync(join(rust, 'crates', 'kicad-wasm', 'tests', 'golden',
                                         'native_reference.json'), 'utf8'));

let checks = 0;

// serde_json writes None as null, serde-wasm-bindgen as undefined: compare JSON-normalised.
const norm = (v) => JSON.parse(JSON.stringify(v, (_k, x) => (x === undefined ? null : x)));
const same = (actual, expected, what) => {
    assert.deepStrictEqual(norm(actual), norm(expected), what);
    checks++;
};

function fnv1a64(str) {
    let h = 0xcbf29ce484222325n;
    for (const b of Buffer.from(str, 'utf8')) {
        h ^= BigInt(b);
        h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
    }
    return h.toString(16).padStart(16, '0');
}

same(k.version(), ref.version, 'version');
same(k.versionInfo(), ref.versionInfo, 'versionInfo');

for (const p of ref.prettify) {
    const out = k.prettify(readFileSync(join(kicad, p.file), 'utf8'));
    same({ length: Buffer.byteLength(out, 'utf8'), fnv: fnv1a64(out) },
         { length: p.length, fnv: p.fnv }, `prettify ${p.file}`);
}

for (const t of ref.tokenStats) {
    same(k.tokenStats(readFileSync(join(kicad, t.file), 'utf8'), t.file), t.stats,
         `tokenStats ${t.file}`);
}

assert.throws(() => k.tokenStats('(a "unterminated', 'x'));
checks++;

for (const s of ref.strokeText)
    same(k.strokeText(s.text, s.sizeMm), s.result, `strokeText ${JSON.stringify(s.text)}`);

for (const b of ref.polygonBoolean)
    same(k.polygonBoolean(b.a, b.b, b.op), b.result, `polygonBoolean ${b.op}`);

assert.throws(() => k.polygonBoolean([], [], 'nope'));
checks++;

same(k.plotDemoGerber(ref.demoGerber.date), ref.demoGerber.text, 'plotDemoGerber');

// loadBoard: the parser has not landed yet -> NotYetPortedError naming it.
const board = readFileSync(join(kicad, 'qa/data/pcbnew/via_stacks.kicad_pcb'), 'utf8');
let loaded = null;
try {
    loaded = k.loadBoard(board, { name: 'via_stacks.kicad_pcb' });
} catch (e) {
    assert.equal(e.name, 'NotYetPortedError', String(e));
    assert.match(e.subsystem, /kicad-pcb/);
    checks++;
}

if (loaded) {
    // Once the parser lands this branch runs; extend it with real comparisons then.
    assert.equal(loaded.name, 'via_stacks.kicad_pcb');
    checks++;
}

// Item access on an empty board (the board model is ported).
const b = k.newBoard('demo.kicad_pcb');
assert.equal(b.name, 'demo.kicad_pcb');
assert.ok(b.nets().some((n) => n.code === 0));
const t = b.addTrack({ start: [0, 0], end: [1000000, 0], width: 250000, layer: 'F.Cu', net: 0 });
const v = b.addVia({ at: [1000000, 0], diameter: 600000, drill: 300000, net: 0 });
let tracks = b.tracks();
assert.equal(tracks.length, 2);
assert.equal(tracks[0].id, t);
assert.equal(tracks[0].kind, 'track');
assert.equal(tracks[1].kind, 'via');
assert.equal(tracks[1].drill, 300000);
assert.equal(tracks[1].bottomLayer, 'B.Cu');
assert.throws(() => b.addTrack({ start: [0, 0], end: [1, 1], width: 1, layer: 'F.SilkS', net: 0 }),
              (e) => e.name === 'InvalidArgumentError');
assert.equal(b.remove(t), true);
assert.equal(b.remove(t), false);
tracks = b.tracks();
assert.equal(tracks.length, 1);
assert.equal(tracks[0].id, v);
checks += 12;

// Not yet ported: every board job names its subsystem. Each call gets a fresh board: a job that
// reaches a kicad-pcb `port_pending!` stub before its own subsystem panics, which is a wasm trap
// (RuntimeError) with panic=abort; that is accepted for the calls marked `stubOk`.
for (const [what, call, stubOk] of [
    ['runDrc', (x) => x.runDrc({}), false],
    ['exportDrill', (x) => x.exportDrill({}), false],
    ['save', (x) => x.save(), false],
    ['ratsnest', (x) => x.ratsnest(), false],
    ['refillZones', (x) => x.refillZones(), false],
    ['exportGerbers', (x) => x.exportGerbers({ layers: 'F.Cu' }), true],
    ['plotSvg', (x) => x.plotSvg({}), true],
]) {
    const fresh = k.newBoard('job.kicad_pcb');
    assert.throws(() => call(fresh),
                  (e) => (e.name === 'NotYetPortedError' && typeof e.subsystem === 'string')
                         || (stubOk && e instanceof WebAssembly.RuntimeError),
                  what);
    checks++;
}

console.log(`smoke.mjs: ${checks} checks passed (wasm results equal the native reference)`);
