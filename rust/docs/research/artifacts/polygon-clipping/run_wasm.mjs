// node run_wasm.mjs <wasm> <which 0|1|2> <cases.txt> <out.txt>
import fs from 'node:fs';
const [wasmPath, whichS, casesPath, outPath] = process.argv.slice(2);
const which = Number(whichS);
const { instance } = await WebAssembly.instantiate(fs.readFileSync(wasmPath), {});
const ex = instance.exports;
const text = fs.readFileSync(casesPath, 'latin1');
const out = fs.openSync(outPath, 'w');
const enc = new TextEncoder();
let pos = 0, n = 0;
const CHUNK = 50;
while (pos < text.length) {
  // take CHUNK cases
  let end = pos;
  for (let i = 0; i < CHUNK; i++) {
    const k = text.indexOf('\nEND\n', end);
    if (k < 0) { end = text.length; break; }
    end = k + 5;
  }
  const bytes = enc.encode(text.slice(pos, end));
  const p = ex.wd_alloc(bytes.length);
  new Uint8Array(ex.memory.buffer, p, bytes.length).set(bytes);
  const len = ex.wd_run(which, p, bytes.length);
  const op = ex.wd_out_ptr();
  fs.writeSync(out, new Uint8Array(ex.memory.buffer, op, len));
  ex.wd_free(p, bytes.length);
  pos = end; n++;
}
fs.closeSync(out);
console.log('chunks', n, 'memory MB', ex.memory.buffer.byteLength / 1048576);
