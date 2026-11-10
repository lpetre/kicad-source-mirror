// This program source code file is part of KiCad, a free EDA CAD application.
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The router loop of an external autorouter on the WebAssembly build (rust/web/pkg-node, made
// by scripts/build-wasm.sh), package WASM-API-1: load a board with its project, record its
// routing through the read API, remove all routing, add it back with one applyEdits call,
// update teardrops when the board has any, refill the zones, run DRC (JSON), save. Every
// result is compared with the native run of the same loop
// (crates/kicad-wasm/tests/golden/router_loop_native.json, test router_loop.rs): native == wasm.
//
//   /opt/node22/bin/node rust/web/test/router_loop.mjs

import { createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const here = dirname(fileURLToPath(import.meta.url));
const rust = join(here, '..', '..');
const kicadRoot = join(rust, '..');
const require = createRequire(import.meta.url);
const k = require(join(rust, 'web', 'pkg-node', 'kicad_wasm.js'));
const golden = JSON.parse(readFileSync(join(rust, 'crates', 'kicad-wasm', 'tests', 'golden',
                                            'router_loop_native.json'), 'utf8'));

function fnv1a64(str) {
    let h = 0xcbf29ce484222325n;
    for (const b of Buffer.from(str, 'utf8')) {
        h ^= BigInt(b);
        h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
    }
    return h.toString(16).padStart(16, '0');
}

const hashed = (text) => ({ length: Buffer.byteLength(text, 'utf8'), fnv: fnv1a64(text) });
const sibling = (path, ext) => {
    const p = path.replace(/\.kicad_pcb$/, ext);
    return existsSync(p) ? readFileSync(p, 'utf8') : undefined;
};

// A recorded route item as the applyEdits addition that re-creates it (UUID included).
function newItem(r) {
    if (r.kind === 'via') {
        const v = r.via;
        return {
            kind: 'via', at: r.start, viaType: v.viaType, top: v.top, bottom: v.bottom,
            diameter: r.width, drill: v.drill, net: r.net, free: v.free, uuid: r.uuid,
            locked: r.locked, finish: v.finish, unconnectedLayers: v.unconnectedLayers,
            zoneLayerConnections: v.zoneLayerConnections, teardrops: v.teardrops,
        };
    }
    const common = {
        start: r.start, end: r.end, width: r.width, layer: r.layer, net: r.net,
        uuid: r.uuid, locked: r.locked, solderMask: r.solderMask ?? false,
        solderMaskMargin: r.solderMaskMargin,
    };
    return r.kind === 'arc' ? { kind: 'arc', mid: r.mid, ...common } : { kind: 'track', ...common };
}

function runLoop(rel) {
    const path = join(kicadRoot, rel);
    const b = k.loadBoard(readFileSync(path, 'utf8'), {
        name: rel.split('/').pop(),
        project: sibling(path, '.kicad_pro'),
        rules: sibling(path, '.kicad_dru'),
        uuidSeed: golden.uuidSeed,
        date: golden.fixedDate,
    });
    const info = b.boardInfo();
    const pads = b.pads({ polygons: true });
    const items = b.routeItems();
    const before = b.connectivity();

    const removed = b.applyEdits({ removeRouting: {} });
    const stripped = b.connectivity();
    const added = b.applyEdits({ add: items.map(newItem) });
    const after = b.connectivity();
    assert.deepStrictEqual(added.addedUuids, items.map((r) => r.uuid), `${rel}: UUIDs kept`);

    let teardrops = null;
    if (b.zones({}).some((z) => z.teardrop)) {
        const o = b.updateTeardrops();
        teardrops = { teardrops: o.teardrops, candidates: o.candidates, without: o.without.length };
    }

    // LIVE-1: the full rebuild a reload of the saved board runs, before refill / DRC / save
    b.finalize();
    b.refillZones();
    const drc = b.runDrc({ format: 'json' });
    const violations = b.drcViolations({});
    const saved = b.save();

    return {
        file: rel,
        copperLayers: info.copperLayerCount,
        pads: pads.length,
        padPolygons: pads.reduce((n, p) => n + (p.polygons ? p.polygons.length : 0), 0),
        items: items.length,
        removed: removed.removed.length,
        added: added.added.length,
        unconnectedBefore: before.unconnected,
        unconnectedStripped: stripped.unconnected,
        unconnectedAfter: after.unconnected,
        teardrops,
        violations: violations.violations.length,
        drc: hashed(drc.report),
        save: hashed(saved),
    };
}

let checks = 0;
for (const expected of golden.boards) {
    const actual = runLoop(expected.file);
    assert.deepStrictEqual(actual, expected, expected.file);
    checks++;
    console.log(`ok ${expected.file}: ${actual.items} items re-added, ${actual.violations} violations`);
}

// Error paths reach JavaScript as typed errors.
{
    const b = k.loadBoard(readFileSync(join(kicadRoot, golden.boards[0].file), 'utf8'),
                          { uuidSeed: golden.uuidSeed, date: golden.fixedDate });
    assert.throws(() => b.applyEdits({ add: [{ kind: 'track', start: [0, 0], end: [1, 1],
                                               width: 1, layer: 'F.SilkS' }] }),
                  (e) => e.name === 'InvalidArgumentError');
    assert.throws(() => b.removeItems([2 ** 60]));
    assert.throws(() => b.constraint({ kind: 'nope', a: { net: 1 }, layer: 'F.Cu' }));
    const c = b.constraint({ kind: 'clearance', a: { net: 1 }, b: { net: 2 }, layer: 'F.Cu' });
    assert.equal(c.constraintType, 'CLEARANCE_CONSTRAINT');
    const q = b.quickDrc({ providers: ['clearance', 'connectivity'] });
    assert.ok(Array.isArray(q.violations));
    checks += 5;
}

// LIVE-1: transactions with typed arrays, checkpoints. Per board: checkpoint, remove all
// routing and add it back from flat records in one commitBulk each, the unconnected count is
// the one of the JSON round trip; rollback restores the base save exactly, and a second run
// after the rollback gives the same reports.
for (const expected of golden.boards.slice(0, 6)) {
    const path = join(kicadRoot, expected.file);
    const b = k.loadBoard(readFileSync(path, 'utf8'), {
        name: expected.file.split('/').pop(), project: sibling(path, '.kicad_pro'),
        rules: sibling(path, '.kicad_dru'), uuidSeed: golden.uuidSeed, date: golden.fixedDate,
    });
    const base = b.save();
    const cp = b.checkpoint();
    const run = () => {
        const rec = b.routeRecords();
        const out = new k.Transaction();
        out.remove(rec.trackIds); out.remove(rec.arcIds); out.remove(rec.viaIds);
        const r1 = b.commitBulk(out);
        assert.ok(r1.removed instanceof Float64Array);
        assert.equal(r1.removed.length, rec.trackIds.length + rec.arcIds.length + rec.viaIds.length);
        assert.equal(r1.unconnected, expected.unconnectedStripped, `${expected.file}: stripped`);
        const back = new k.Transaction();
        back.append = true;
        back.addTracks(rec.tracks); back.addArcs(rec.arcs); back.addVias(rec.vias);
        const r2 = b.commitBulk(back);
        assert.equal(r2.added.length, r1.removed.length);
        const again = b.routeRecords();
        assert.deepStrictEqual(Array.from(again.tracks), Array.from(rec.tracks));
        assert.deepStrictEqual(Array.from(again.vias), Array.from(rec.vias));
        return [r1.unconnected, r2.unconnected, Array.from(r2.dirtyNets).join(','), b.save()];
    };
    const first = run();
    b.rollback(cp);
    assert.equal(b.save(), base, `${expected.file}: save after rollback`);
    const second = run();
    assert.deepStrictEqual(second, first, `${expected.file}: second run after rollback`);
    b.rollback(cp);
    assert.equal(b.releaseCheckpoint(cp), true);
    checks += 4;
}

console.log(`router_loop: ${checks} checks passed`);
