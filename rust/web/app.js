// KiCad Rust port: WebAssembly demo page logic.
// Copyright The KiCad Developers, see AUTHORS.txt for contributors. GPL-3.0-or-later.

import init, * as kicad from './pkg/kicad_wasm.js';

const $ = (id) => document.getElementById(id);
let board = null;

function showStats(name, text) {
    const t0 = performance.now();
    let stats;

    try {
        stats = kicad.tokenStats(text, name);
    } catch (e) {
        $('stats-empty').textContent = `Lexer error: ${e.message}`;
        $('stats').hidden = true;
        return;
    }

    const pretty = kicad.prettify(text);
    const ms = (performance.now() - t0).toFixed(1);
    const rows = [
        ['File', name],
        ['Bytes', text.length.toLocaleString()],
        ['Lines', stats.lines.toLocaleString()],
        ['Tokens', stats.tokens.toLocaleString()],
        ['Lists', stats.lists.toLocaleString()],
        ['Keywords', stats.keywords.toLocaleString()],
        ['Other symbols', stats.symbols.toLocaleString()],
        ['Numbers', stats.numbers.toLocaleString()],
        ['Strings', stats.strings.toLocaleString()],
        ['Max depth', stats.maxDepth],
        ['Top keywords', stats.topKeywords.map(([k, n]) => `${k} ${n}`).join(', ')],
    ];
    const table = $('stats');
    table.replaceChildren(...rows.map(([k, v]) => {
        const tr = document.createElement('tr');
        const th = document.createElement('th');
        const td = document.createElement('td');
        th.textContent = k;
        td.textContent = v;
        tr.append(th, td);
        return tr;
    }));
    table.hidden = false;
    $('stats-empty').hidden = true;
    $('pretty').textContent =
        `Prettified size: ${pretty.length.toLocaleString()} characters (lexed and prettified in ${ms} ms).`;
    document.body.dataset.statsTokens = String(stats.tokens);
}

function tryLoadBoard(name, text) {
    board = null;
    $('drc').disabled = true;
    $('gerbers').disabled = true;

    try {
        board = kicad.loadBoard(text, { name });
        $('board-result').textContent =
            `Loaded ${board.name}: ${board.nets().length} nets, ${board.tracks().length} tracks.`;
        $('drc').disabled = false;
        $('gerbers').disabled = false;
    } catch (e) {
        $('board-result').textContent = e.name === 'NotYetPortedError'
            ? `Board loading is not available yet (${e.subsystem}).`
            : `Could not load the board: ${e.message}`;
    }
}

async function handleFile(file) {
    const text = await file.text();
    showStats(file.name, text);
    tryLoadBoard(file.name, text);
}

function runJob(button, out, fn) {
    $(button).addEventListener('click', () => {
        try {
            const r = fn();
            $(out).textContent = `exit code ${r.exitCode}, ${r.files.length} file(s)`;
        } catch (e) {
            $(out).textContent = e.name === 'NotYetPortedError'
                ? `not yet ported: ${e.subsystem}` : e.message;
        }
    });
}

function strokeDemo() {
    const r = kicad.strokeText('KiCad + Rust', 2.0);
    const ns = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(ns, 'svg');
    // stroke text is drawn bottom-left aligned at the origin, y down (board units, nm)
    svg.setAttribute('viewBox', '-500000 -3000000 26000000 3800000');
    svg.setAttribute('width', '520');
    const path = document.createElementNS(ns, 'path');
    path.setAttribute('d', r.path);
    path.setAttribute('fill', 'none');
    path.setAttribute('stroke', 'currentColor');
    path.setAttribute('stroke-width', String(r.strokeWidth));
    path.setAttribute('stroke-linecap', 'round');
    path.setAttribute('stroke-linejoin', 'round');
    svg.append(path);
    $('stroke').replaceChildren(svg);
}

async function main() {
    await init();
    $('status').textContent = `loaded, KiCad ${kicad.version()} (${kicad.versionInfo().build})`;
    document.body.dataset.wasmVersion = kicad.version();

    $('demo-gerber').textContent = kicad.plotDemoGerber();
    strokeDemo();

    const drop = $('drop');
    drop.addEventListener('click', () => $('file').click());
    $('file').addEventListener('change', (e) => e.target.files[0] && handleFile(e.target.files[0]));
    drop.addEventListener('dragover', (e) => { e.preventDefault(); drop.classList.add('over'); });
    drop.addEventListener('dragleave', () => drop.classList.remove('over'));
    drop.addEventListener('drop', (e) => {
        e.preventDefault();
        drop.classList.remove('over');
        if (e.dataTransfer.files[0]) handleFile(e.dataTransfer.files[0]);
    });

    runJob('drc', 'drc-result', () => board.runDrc({}));
    runJob('gerbers', 'gerbers-result', () => board.exportGerbers({}));

    document.body.dataset.ready = '1';
}

// Exposed for tests: feed a file without a real drag and drop.
window.kicadDemo = { handleText: (name, text) => { showStats(name, text); tryLoadBoard(name, text); } };

main().catch((e) => {
    $('status').textContent = `failed: ${e}`;
    document.body.dataset.error = String(e);
});
