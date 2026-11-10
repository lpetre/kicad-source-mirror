// This program source code file is part of KiCad, a free EDA CAD application.
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// wasm half of the PERF-1 baseline (docs/PERF.md): the kicad-bench stages through the
// JavaScript API of the WebAssembly build (rust/web/pkg-node, made by scripts/build-wasm.sh
// with the wasm-release profile) under node.
//
//   node wasm_bench.mjs [--reps N] [--warmup W] [--pkg DIR] [--commit HASH] BOARD...
//
// Pipelines as in kicad-bench (each from a fresh loadBoard): drc (load, runDrc), fab (load,
// exportGerbers, exportDrill, save), refill (load, refillZones, runDrc). Prints TSV lines:
// commit, board, stage, n, min_ms, median_ms, max_ms, peak_rss_mb (node process, '-' per
// stage), allocs ('-'), alloc_mb ('-').

import { createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join, basename, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';

const here = dirname(fileURLToPath(import.meta.url));
const rust = join(here, '..', '..', '..');
const args = process.argv.slice(2);
let reps = 7, warmup = 1, pkg = join(rust, 'web', 'pkg-node'), commit = '-';
const boards = [];

while (args.length) {
    const a = args.shift();
    if (a === '--reps') reps = Number(args.shift());
    else if (a === '--warmup') warmup = Number(args.shift());
    else if (a === '--pkg') pkg = resolve(args.shift());
    else if (a === '--commit') commit = args.shift();
    else boards.push(resolve(a));
}

const require = createRequire(import.meta.url);
const k = require(join(pkg, 'kicad_wasm.js'));
const DATE = '2026-01-01T00:00:00';

const side = (board, ext) => {
    const p = board.replace(/\.kicad_pcb$/, ext);
    return existsSync(p) ? readFileSync(p, 'utf8') : undefined;
};

const median = (v) => {
    const s = [...v].sort((x, y) => x - y);
    const n = s.length;
    return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2;
};

console.log('commit\tboard\tstage\tn\tmin_ms\tmedian_ms\tmax_ms\tpeak_rss_mb\tallocs\talloc_mb');

for (const board of boards) {
    const label = basename(board, '.kicad_pcb');
    const text = readFileSync(board, 'utf8');
    const opts = {
        project: side(board, '.kicad_pro'),
        localSettings: side(board, '.kicad_prl'),
        rules: side(board, '.kicad_dru'),
        name: board,
        uuidSeed: 1,
        date: DATE,
    };
    const samples = new Map();
    const failed = new Set();
    let keep = false;

    const stage = (name, f) => {
        const t0 = performance.now();
        const r = f();
        const ms = performance.now() - t0;
        if (keep) {
            if (!samples.has(name)) samples.set(name, []);
            samples.get(name).push(ms);
        }
        return r;
    };

    const pipelines = {
        drc: () => {
            const b = stage('load_total', () => k.loadBoard(text, opts));
            stage('drc', () => b.runDrc({}));
            b.free();
        },
        fab: () => {
            const b = stage('load_total', () => k.loadBoard(text, opts));
            stage('gerbers', () => b.exportGerbers({}));
            stage('drill', () => b.exportDrill({}));
            stage('write', () => b.save());
            b.free();
        },
        refill: () => {
            const b = stage('load_total', () => k.loadBoard(text, opts));
            stage('zone_refill', () => b.refillZones());
            stage('drc_after_refill', () => b.runDrc({}));
            b.free();
        },
    };

    for (let rep = 0; rep < warmup + reps; rep++) {
        keep = rep >= warmup;
        for (const [name, run] of Object.entries(pipelines)) {
            if (failed.has(name)) continue;
            try {
                run();
            } catch (e) {
                console.error(`wasm_bench: ${label}: pipeline ${name}: ${e}`);
                failed.add(name);
            }
        }
    }

    const rss = (process.resourceUsage().maxRSS / 1024).toFixed(1);
    for (const [name, v] of samples) {
        const s = [...v].sort((x, y) => x - y);
        console.log(`${commit}\t${label}\t${name}\t${s.length}\t${s[0].toFixed(2)}\t` +
                    `${median(s).toFixed(2)}\t${s[s.length - 1].toFixed(2)}\t${rss}\t-\t-`);
    }
    for (const name of failed)
        console.log(`${commit}\t${label}\t${name}\tFAILED\t-\t-\t-\t-\t-\t-`);
}
