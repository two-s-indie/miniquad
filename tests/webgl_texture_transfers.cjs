const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const sandbox = { console, document: { querySelector: () => ({ focus() {} }) }, nodeAssert: assert };
vm.createContext(sandbox);
vm.runInContext(fs.readFileSync(process.argv[2] || path.join(__dirname, '../js/gl.js'), 'utf8'), sandbox);
vm.runInContext(`
wasm_memory = { buffer: new ArrayBuffer(256) };
let received;
gl = {
    ALPHA: 0x1906, RGB: 0x1907, RGBA: 0x1908,
    texImage2D(...args) { received = args[8]; },
    texSubImage2D(...args) { received = args[8]; },
    readPixels(...args) { received = args[6]; received.fill(0x3C00); }
};
const env = importObject.env;
const rgba = 0x1908, half = 0x140B, float = 0x1406;
new Uint16Array(wasm_memory.buffer, 16, 4).set([0x3C00, 0, 0x3800, 0x3C00]);
env.glTexImage2D(0, 0, 0x881A, 1, 1, 0, rgba, half, 16);
nodeAssert.ok(received instanceof Uint16Array);
nodeAssert.equal(received.byteLength, 8);
nodeAssert.deepEqual(Array.from(received), [0x3C00, 0, 0x3800, 0x3C00]);
env.glTexSubImage2D(0, 0, 0, 0, 1, 1, rgba, half, 16);
nodeAssert.ok(received instanceof Uint16Array);
nodeAssert.equal(received.byteLength, 8);
env.glReadPixels(0, 0, 1, 1, rgba, half, 16);
nodeAssert.ok(received instanceof Uint16Array);
nodeAssert.deepEqual(Array.from(new Uint16Array(wasm_memory.buffer, 16, 4)), [0x3C00, 0x3C00, 0x3C00, 0x3C00]);

// Rust byte slices may start at an odd address. Uploads and readback must still work.
new Uint8Array(wasm_memory.buffer, 33, 8).set([0, 60, 0, 0, 0, 56, 0, 60]);
env.glTexImage2D(0, 0, 0x881A, 1, 1, 0, rgba, half, 33);
nodeAssert.deepEqual(Array.from(received), [0x3C00, 0, 0x3800, 0x3C00]);
env.glReadPixels(0, 0, 1, 1, rgba, half, 33);
nodeAssert.deepEqual(Array.from(new Uint8Array(wasm_memory.buffer, 33, 8)), [0, 60, 0, 60, 0, 60, 0, 60]);
nodeAssert.equal(new Uint8Array(wasm_memory.buffer)[41], 0);

env.glTexImage2D(0, 0, 0x881A, 1, 1, 0, rgba, half, 0);
nodeAssert.equal(received, null);
env.glTexImage2D(0, 0, rgba, 1, 1, 0, rgba, 0x1401, 16);
nodeAssert.ok(received instanceof Uint8Array);
nodeAssert.equal(received.byteLength, 4);
env.glTexImage2D(0, 0, rgba, 1, 1, 0, rgba, float, 16);
nodeAssert.ok(received instanceof Float32Array);
nodeAssert.equal(received.byteLength, 16);
env.glTexImage2D(0, 0, 0x1902, 1, 1, 0, 0x1902, 0x1403, 16);
nodeAssert.ok(received instanceof Uint16Array);
nodeAssert.equal(received.byteLength, 2);
`, sandbox);
console.log('WebGL texture transfer regression checks passed');
