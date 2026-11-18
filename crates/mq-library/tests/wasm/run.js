'use strict';
// Host for mq-library's wasm selftest (wasm-test.sh), CommonJS for Node 8.12
// to 20: the service's own host.js (the Node 8 globals and the file calls the
// VFS imports). In the `cut` phase the process exits when the database file
// is first synced (its pages written, the journal still hot), as a power cut
// would.
// Usage: node run.js <glue.js> <dir> <write|cut|read>
const path = require('path');
const host = require('../../../../apps/webos/service/host.js');

const glue = process.argv[2];
const dir = process.argv[3];
const phase = process.argv[4];
const tag = process.version + ' ' + phase;
const fail = (what) => {
  console.log('FAIL ' + tag + ': ' + what);
  process.exit(1);
};

let core;
host.install(() => core);
host.setOnDied((message) => fail('died: ' + message));

if (phase === 'cut') {
  const h = globalThis.mqHost;
  const fsOpen = h.fsOpen;
  const fsSync = h.fsSync;
  const names = new Map();
  h.fsOpen = (p, create) => {
    const fd = fsOpen(p, create);
    names.set(fd, p);
    return fd;
  };
  // Every page of the new transaction is in the database file, the commit
  // has not finished, and the power goes: only the hot journal can undo it.
  h.fsSync = (fd) => {
    if (/library\.sqlite$/.test(names.get(fd) || '')) {
      console.log('CUT ' + tag);
      process.exit(3);
    }
    return fsSync(fd);
  };
}

core = require(path.resolve(glue));
console.log('PASS ' + tag + ': ' + core.selftest(path.resolve(dir), phase));
