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

// Item access on an empty board (the board model is ported).
const b = k.newBoard('demo.kicad_pcb', { uuidSeed: ref.uuidSeed, date: ref.fixedDate });
assert.equal(b.name, 'demo.kicad_pcb');
assert.ok(b.nets().some((n) => n.code === 0));
const t = b.addTrack({ start: [0, 0], end: [1000000, 0], width: 250000, layer: 'F.Cu', net: 0 });
const v = b.addVia({ at: [1000000, 0], diameter: 600000, drill: 300000, net: 0 });
let tracks = b.tracks();
same(tracks, ref.itemBoard.tracks, 'newBoard tracks (ids, uuids, geometry)');
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
same(b.nets(), ref.itemBoard.nets, 'newBoard nets');
{
    const saved = b.save();
    same({ length: Buffer.byteLength(saved, 'utf8'), fnv: fnv1a64(saved) }, ref.itemBoard.save,
         'newBoard save');
}

// loadBoard on the corpus boards the parser and writer support in full: the item lists and the
// saved text equal the native results (and the native save equals the reference kicad-cli).
for (const l of ref.loadBoard) {
    const lb = k.loadBoard(readFileSync(join(kicad, l.file), 'utf8'), { name: l.name });
    assert.equal(lb.name, l.name);
    same(lb.nets(), l.nets, `loadBoard ${l.file} nets`);
    same(lb.tracks(), l.tracks, `loadBoard ${l.file} tracks`);
    const saved = lb.save();
    same({ length: Buffer.byteLength(saved, 'utf8'), fnv: fnv1a64(saved) }, l.save,
         `loadBoard ${l.file} save`);
}

// The board jobs (DRC report and JSON, Gerbers, Excellon and Gerber drill files) on corpus
// boards with their project and rule files, fixed UUID seed and date, one fresh load per job:
// exit code, messages and every produced file equal the native run (whose CLI counterpart is
// compared with the reference kicad-cli by kicad-cli's e2e test).
const reduce = (r) => ({
    exitCode: r.exitCode,
    files: r.files.map((f) => ({ name: f.name,
                                 content: { length: Buffer.byteLength(f.content, 'utf8'),
                                            fnv: fnv1a64(f.content) } })),
    report: r.report == null ? null : { length: Buffer.byteLength(r.report, 'utf8'),
                                        fnv: fnv1a64(r.report) },
    messages: r.messages,
});
const readOpt = (p) => { try { return readFileSync(p, 'utf8'); } catch { return undefined; } };
for (const j of ref.jobs) {
    const path = join(kicad, j.file);
    const load = () => k.loadBoard(readFileSync(path, 'utf8'), {
        name: j.file.split('/').pop(),
        project: readOpt(path.replace(/\.kicad_pcb$/, '.kicad_pro')),
        rules: readOpt(path.replace(/\.kicad_pcb$/, '.kicad_dru')),
        uuidSeed: ref.uuidSeed,
        date: ref.fixedDate,
    });
    same(reduce(load().runDrc({})), j.drcReport, `runDrc ${j.file}`);
    same(reduce(load().runDrc({ format: 'json', severities: ['error', 'warning', 'exclusions'] })),
         j.drcJson, `runDrc json ${j.file}`);
    same(reduce(load().exportGerbers({})), j.gerbers, `exportGerbers ${j.file}`);
    same(reduce(load().exportDrill({})), j.drillExcellon, `exportDrill ${j.file}`);
    same(reduce(load().exportDrill({ format: 'gerber' })), j.drillGerber,
         `exportDrill gerber ${j.file}`);
}

// plotSvg (FAB-11 is wiring it): SVG text, or an error naming the subsystem.
{
    const fresh = k.newBoard('job.kicad_pcb');
    let svg = null;
    try {
        svg = fresh.plotSvg({});
    } catch (e) {
        assert.ok((e.name === 'NotYetPortedError' && typeof e.subsystem === 'string')
                  || e instanceof WebAssembly.RuntimeError, String(e));
    }
    if (svg !== null) assert.match(svg, /<svg/);
    checks++;
}

// loadBoard: the board parser is landing package by package. Until every construct of the
// corpus board is ported the call fails with NotYetPortedError (a staging stub of the parser)
// or traps in a kicad-pcb `port_pending!` stub (RuntimeError, panic=abort). It runs at
// the end of the test so that a trap cannot affect the other checks.
const board = readFileSync(join(kicad, 'qa/data/pcbnew/via_stacks.kicad_pcb'), 'utf8');
{
    let loaded = null;
    try {
        loaded = k.loadBoard(board, { name: 'via_stacks.kicad_pcb' });
    } catch (e) {
        assert.ok(e.name === 'NotYetPortedError' || e.name === 'LoadError'
                  || e instanceof WebAssembly.RuntimeError, String(e));
        if (e.name === 'NotYetPortedError') assert.match(e.subsystem, /kicad-/);
    }
    if (loaded) assert.equal(loaded.name, 'via_stacks.kicad_pcb');
    checks++;
}

console.log(`smoke.mjs: ${checks} checks passed (wasm results equal the native reference)`);
