// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.
//
// REV-FAB review harness (rust/docs/design/review-fab.md). Scratch output goes to
// rust/target/rev-fab/work; the reference kicad-cli always runs on copies.
// REV-FAB: export gerbers + drill of one board through the wasm module (rust/web/pkg-node) into OUTDIR.
//   node wasm_export.mjs BOARD OUTDIR [gerberOptionsJson] [drillOptionsJson]
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
const require = createRequire(import.meta.url);
const k = require('/home/user/kicad-source-mirror/rust/web/pkg-node/kicad_wasm.js');
const [board, outdir, gopt, dopt] = process.argv.slice(2);
const stem = basename(board).replace(/\.kicad_pcb$/, '');
const opt = { name: basename(board), uuidSeed: 1, date: '2026-01-02T03:04:05' };
for (const [ext, key] of [['.kicad_pro', 'project'], ['.kicad_dru', 'rules'], ['.kicad_prl', 'localSettings']]) {
    const p = join(dirname(board), stem + ext);
    if (existsSync(p)) opt[key] = readFileSync(p, 'utf8');
}
mkdirSync(join(outdir, 'g'), { recursive: true });
mkdirSync(join(outdir, 'd'), { recursive: true });
const put = (sub, f) => {
    const c = f.content ?? f.text ?? f.contents;
    writeFileSync(join(outdir, sub, basename(f.name)), typeof c === 'string' ? c : Buffer.from(c));
};
try {
    const b = k.loadBoard(readFileSync(board, 'utf8'), opt);
    const g = b.exportGerbers(gopt ? JSON.parse(gopt) : {});
    for (const f of g.files) put('g', f);
    console.log('gerbers exit', g.exitCode, 'files', g.files.length);
} catch (e) {
    console.log('GERBER ERROR', e && (e.name + ': ' + e.message));
}
try {
    const b2 = k.loadBoard(readFileSync(board, 'utf8'), opt);
    const d = b2.exportDrill(dopt ? JSON.parse(dopt) : {});
    for (const f of d.files) put('d', f);
    console.log('drill exit', d.exitCode, 'files', d.files.length);
} catch (e) {
    console.log('DRILL ERROR', e && (e.name + ': ' + e.message));
}
