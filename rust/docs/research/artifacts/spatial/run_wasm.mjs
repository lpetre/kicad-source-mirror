// Run the hand-port checksums inside WebAssembly (wasm32-unknown-unknown, no imports).
import { readFileSync } from 'node:fs';
const bytes = readFileSync(process.argv[2]);
const mod = new WebAssembly.Module(bytes);
const imports = {};
for (const imp of WebAssembly.Module.imports(mod)) {
  console.log('unexpected import', imp);
}
const inst = new WebAssembly.Instance(mod, imports);
const u64 = (v) => BigInt.asUintN(64, v).toString();
console.log('delaunay_checksum     =', u64(inst.exports.delaunay_checksum()));
console.log('packed_rtree_checksum =', u64(inst.exports.packed_rtree_checksum()));
console.log('nanoflann_checksum    =', u64(inst.exports.nanoflann_checksum()));
console.log('dynamic_rtree_checksum=', u64(inst.exports.dynamic_rtree_checksum()));
