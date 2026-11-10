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

function tryLoadBoard(name, text, project, rules) {
    board = null;
    for (const id of ['drc', 'gerbers', 'svg']) $(id).disabled = true;

    try {
        board = kicad.loadBoard(text, { name, project, rules });
        const warn = board.warnings.length ? ` ${board.warnings.length} font warning(s).` : '';
        $('board-result').textContent =
            `Loaded ${board.name}${project ? ' with its project' : ''}${rules ? ' and rules' : ''}: `
            + `${board.nets().length} nets, ${board.tracks().length} tracks and vias.${warn}`;
        for (const id of ['drc', 'gerbers', 'svg']) $(id).disabled = false;
    } catch (e) {
        $('board-result').textContent = e.name === 'NotYetPortedError'
            ? `Board loading is not available yet (${e.subsystem}).`
            : `Could not load the board: ${e.message}`;
    }
}

// The dropped files: the first .kicad_pcb, and the .kicad_pro / .kicad_dru next to it.
async function handleFiles(files) {
    const list = [...files];
    const pcb = list.find((f) => f.name.endsWith('.kicad_pcb')) ?? list[0];
    if (!pcb) return;
    const stem = pcb.name.replace(/\.[^.]*$/, '');
    const sibling = async (ext) => {
        const f = list.find((x) => x.name === stem + ext);
        return f ? f.text() : undefined;
    };
    const text = await pcb.text();
    showStats(pcb.name, text);
    tryLoadBoard(pcb.name, text, await sibling('.kicad_pro'), await sibling('.kicad_dru'));
}

// CRC-32 (IEEE) and a store-only zip writer for the download.
const CRC_TABLE = (() => {
    const t = new Uint32Array(256);
    for (let n = 0; n < 256; n++) {
        let c = n;
        for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
        t[n] = c >>> 0;
    }
    return t;
})();

function crc32(bytes) {
    let c = 0xffffffff;
    for (const b of bytes) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
}

function zip(files) {
    const enc = new TextEncoder();
    const parts = [];
    const central = [];
    let offset = 0;
    for (const f of files) {
        const name = enc.encode(f.name);
        const data = enc.encode(f.content);
        const crc = crc32(data);
        const local = new DataView(new ArrayBuffer(30));
        local.setUint32(0, 0x04034b50, true);
        local.setUint16(4, 20, true);
        local.setUint32(14, crc, true);
        local.setUint32(18, data.length, true);
        local.setUint32(22, data.length, true);
        local.setUint16(26, name.length, true);
        parts.push(new Uint8Array(local.buffer), name, data);
        const cd = new DataView(new ArrayBuffer(46));
        cd.setUint32(0, 0x02014b50, true);
        cd.setUint16(4, 20, true);
        cd.setUint16(6, 20, true);
        cd.setUint32(16, crc, true);
        cd.setUint32(20, data.length, true);
        cd.setUint32(24, data.length, true);
        cd.setUint16(28, name.length, true);
        cd.setUint32(42, offset, true);
        central.push(new Uint8Array(cd.buffer), name);
        offset += 30 + name.length + data.length;
    }
    const size = central.reduce((n, p) => n + p.length, 0);
    const end = new DataView(new ArrayBuffer(22));
    end.setUint32(0, 0x06054b50, true);
    end.setUint16(8, files.length, true);
    end.setUint16(10, files.length, true);
    end.setUint32(12, size, true);
    end.setUint32(16, offset, true);
    return new Blob([...parts, ...central, new Uint8Array(end.buffer)], { type: 'application/zip' });
}

function showError(out, e) {
    $(out).textContent = e.name === 'NotYetPortedError' ? `not yet ported: ${e.subsystem}`
        : e instanceof WebAssembly.RuntimeError ? `internal error (see the console): ${e.message}`
        : e.message;
}

function runDrc() {
    try {
        const r = board.runDrc({});
        const count = (re) => (r.report.match(re) || [null, '0'])[1];
        $('drc-result').textContent = `${count(/Found (\d+) DRC violations/)} violations, `
            + `${count(/Found (\d+) unconnected pads/)} unconnected pads`;
        $('drc-report').textContent = r.report;
        $('drc-report').hidden = false;
        window.kicadDemo.lastDrc = r;
    } catch (e) {
        showError('drc-result', e);
    }
}

function exportFab() {
    try {
        const gerbers = board.exportGerbers({});
        const drill = board.exportDrill({});
        const files = [...gerbers.files, ...drill.files];
        $('fab-files').replaceChildren(...files.map((f) => {
            const li = document.createElement('li');
            li.textContent = `${f.name} (${f.content.length.toLocaleString()} bytes)`;
            return li;
        }));
        const a = document.createElement('a');
        a.href = URL.createObjectURL(zip(files));
        a.download = board.name.replace(/\.kicad_pcb$/, '') + '-fab.zip';
        a.textContent = `download ${a.download}`;
        $('gerbers-result').replaceChildren(`${files.length} files `, a);
        window.kicadDemo.lastFab = files;
    } catch (e) {
        showError('gerbers-result', e);
    }
}

function plotSvg() {
    try {
        const svg = board.plotSvg({ layers: 'F.Cu', fitPageToBoard: true });
        $('svg-view').innerHTML = svg;
        $('svg-result').textContent = '';
    } catch (e) {
        showError('svg-result', e);
    }
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
    $('file').addEventListener('change', (e) => handleFiles(e.target.files));
    drop.addEventListener('dragover', (e) => { e.preventDefault(); drop.classList.add('over'); });
    drop.addEventListener('dragleave', () => drop.classList.remove('over'));
    drop.addEventListener('drop', (e) => {
        e.preventDefault();
        drop.classList.remove('over');
        handleFiles(e.dataTransfer.files);
    });

    $('drc').addEventListener('click', runDrc);
    $('gerbers').addEventListener('click', exportFab);
    $('svg').addEventListener('click', plotSvg);

    document.body.dataset.ready = '1';
}

// Exposed for tests: feed a file without a real drag and drop.
// Exposed for tests: feed files without a real drag and drop; `loadWith` takes loadBoard options
// (e.g. a fixed uuidSeed and date for a comparison with kicad-cli).
window.kicadDemo = {
    handleText: (name, text, project, rules) => {
        showStats(name, text);
        tryLoadBoard(name, text, project, rules);
    },
    loadWith: (name, text, options) => {
        board = kicad.loadBoard(text, { name, ...options });
        for (const id of ['drc', 'gerbers', 'svg']) $(id).disabled = false;
    },
    runDrc, exportFab, lastDrc: null, lastFab: null,
};

main().catch((e) => {
    $('status').textContent = `failed: ${e}`;
    document.body.dataset.error = String(e);
});
