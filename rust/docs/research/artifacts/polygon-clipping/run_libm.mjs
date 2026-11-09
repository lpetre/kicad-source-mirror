// node run_libm.mjs <wasm> <native_trace.txt>
import fs from 'node:fs';
const [wasmPath, nativePath] = process.argv.slice(2);
const { instance } = await WebAssembly.instantiate(fs.readFileSync(wasmPath), {});
const ex = instance.exports;
const len = ex.wd_libm_trace();
const w = Buffer.from(new Uint8Array(ex.memory.buffer, ex.wd_out_ptr(), len)).toString('latin1').split('\n');
const n = fs.readFileSync(nativePath, 'latin1').split('\n');
if (w.length !== n.length) console.log('LENGTH MISMATCH', w.length, n.length);
let name = '', tot = 0, bad = 0, maxUlp = 0n, rows = [];
const flush = () => { if (name) rows.push([name, tot, bad, maxUlp]); };
for (let i = 0; i < n.length; i++) {
  if (n[i].startsWith('#')) { flush(); name = n[i].slice(2); tot = 0; bad = 0; maxUlp = 0n; continue; }
  if (!n[i]) continue;
  tot++;
  if (n[i] !== w[i]) {
    bad++;
    let d = BigInt('0x' + n[i]) - BigInt('0x' + w[i]); if (d < 0n) d = -d;
    if (d > maxUlp) maxUlp = d;
  }
}
flush();
console.log('section'.padEnd(36), 'values'.padStart(8), 'differ'.padStart(8), 'max_ulp_diff'.padStart(13));
for (const [a, b, c, d] of rows) console.log(a.padEnd(36), String(b).padStart(8), String(c).padStart(8), String(d).padStart(13));
