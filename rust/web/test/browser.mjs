// This program source code file is part of KiCad, a free EDA CAD application.
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Browser test of the demo page: serves rust/web from a local static server, opens index.html
// in the pre-installed Chromium (Playwright), and checks that the page loads the wasm module,
// shows the version, the demo Gerber and the stroke text, and computes token statistics for a
// corpus board fed through the page's file handler (the same numbers as the native reference).
//
//   /opt/node22/bin/node rust/web/test/browser.mjs

import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, statSync, mkdtempSync, writeFileSync, readdirSync, rmSync }
    from 'node:fs';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { dirname, join, extname, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const here = dirname(fileURLToPath(import.meta.url));
const web = join(here, '..');
const rust = join(web, '..');
const kicad = join(rust, '..');
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const TYPES = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.mjs': 'text/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json',
};

const server = createServer((req, res) => {
    const url = new URL(req.url, 'http://localhost');
    const rel = normalize(decodeURIComponent(url.pathname)).replace(/^([/\\])+/, '');
    const file = join(web, rel === '' ? 'index.html' : rel);

    if (!file.startsWith(web) || !existsSync(file) || statSync(file).isDirectory()) {
        res.writeHead(404);
        res.end('not found');
        return;
    }

    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' });
    res.end(readFileSync(file));
});

await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;
const browser = await chromium.launch();

try {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));

    await page.goto(`http://127.0.0.1:${port}/index.html`);
    await page.waitForFunction(() => document.body.dataset.ready === '1' || document.body.dataset.error,
                               null, { timeout: 30000 });

    const error = await page.evaluate(() => document.body.dataset.error);
    assert.equal(error, undefined, `page failed: ${error}`);
    assert.deepEqual(errors, []);

    const ref = JSON.parse(readFileSync(join(rust, 'crates/kicad-wasm/tests/golden/native_reference.json'),
                                        'utf8'));
    assert.equal(await page.evaluate(() => document.body.dataset.wasmVersion), ref.version);
    assert.match(await page.textContent('#status'), /^loaded, KiCad /);
    // plotDemoGerber() without a date uses the reference's fixed date
    assert.equal(await page.textContent('#demo-gerber'), ref.demoGerber.text);
    assert.ok(await page.locator('#stroke path').count() === 1);

    const entry = ref.tokenStats[0];
    const text = readFileSync(join(kicad, entry.file), 'utf8');
    await page.evaluate(([n, t]) => window.kicadDemo.handleText(n, t), [entry.file, text]);
    assert.equal(await page.evaluate(() => document.body.dataset.statsTokens),
                 String(entry.stats.tokens));
    assert.match(await page.textContent('#board-result'), /not available yet|Loaded|Could not load/);

    // The autorouter use case end to end: a board with its project loaded in the page, DRC and
    // the fabrication outputs (Gerbers + Excellon), compared with the native kicad-rs CLI run on
    // the same files with the same fixed date and UUID seed (kicad-rs itself is compared with
    // the reference kicad-cli by crates/kicad-cli/tests/oracle/e2e_cli.py).
    const cli = join(rust, 'target', 'debug', 'kicad-rs');
    let fabChecked = 0;
    if (existsSync(cli)) {
        const rel = 'demos/ecc83/ecc83-pp.kicad_pcb';
        const name = 'ecc83-pp.kicad_pcb';
        const pcb = readFileSync(join(kicad, rel), 'utf8');
        const pro = readFileSync(join(kicad, rel.replace(/\.kicad_pcb$/, '.kicad_pro')), 'utf8');
        const opts = { project: pro, uuidSeed: 7, date: '2026-01-02T03:04:05' };
        await page.evaluate(([n, t, o]) => window.kicadDemo.loadWith(n, t, o), [name, pcb, opts]);
        await page.click('#drc');
        await page.click('#gerbers');
        const drc = await page.evaluate(() => window.kicadDemo.lastDrc);
        const fab = await page.evaluate(() => window.kicadDemo.lastFab);
        assert.ok(drc && fab && fab.length > 0, 'page ran DRC and the export');
        assert.match(await page.textContent('#gerbers-result'), /download ecc83-pp-fab\.zip/);

        const work = mkdtempSync(join(tmpdir(), 'kicad-wasm-browser-'));
        try {
            writeFileSync(join(work, name), pcb);
            writeFileSync(join(work, 'ecc83-pp.kicad_pro'), pro);
            const env = { ...process.env, KICAD_RS_DATE: opts.date, KICAD_RS_UUID_SEED: '7' };
            const run = (...args) => execFileSync(cli, args, { cwd: work, env, stdio: 'pipe' });
            run('pcb', 'drc', '--format', 'report', '-o', 'drc.rpt', name);
            run('pcb', 'export', 'gerbers', '-o', 'out/', name);
            run('pcb', 'export', 'drill', '-o', 'out/', name);
            assert.equal(drc.report, readFileSync(join(work, 'drc.rpt'), 'utf8'), 'DRC report');
            const cliFiles = readdirSync(join(work, 'out')).sort();
            assert.deepEqual(fab.map((f) => f.name).sort(), cliFiles, 'fabrication file names');
            for (const f of fab) {
                assert.equal(f.content, readFileSync(join(work, 'out', f.name), 'utf8'), f.name);
                fabChecked++;
            }
        } finally {
            rmSync(work, { recursive: true, force: true });
        }
    }

    console.log(`browser.mjs: page loaded the wasm module (KiCad ${ref.version}), ` +
                `token statistics of ${entry.file}: ${entry.stats.tokens} tokens; ` +
                (fabChecked ? `DRC report and ${fabChecked} Gerber / drill files equal kicad-rs`
                            : 'kicad-rs not built: CLI comparison skipped'));
} finally {
    await browser.close();
    server.close();
}
