'use strict';
// Host for mq-rt's wasm selftest (wasm-test.sh), CommonJS for Node 8.12 to
// 20: the globals the glue needs on Node 8, and the mqHost calls mq-rt
// imports. MQ_EARLY=1 fires every timer at half its delay, which a sleep
// must survive by re-arming. Usage: node run.js <glue.js>
if (typeof globalThis === 'undefined') global.globalThis = global;
if (typeof TextDecoder === 'undefined') {
  global.TextDecoder = require('util').TextDecoder;
  global.TextEncoder = require('util').TextEncoder;
}
if (typeof globalThis.performance === 'undefined') {
  globalThis.performance = require('perf_hooks').performance;
}

const early = process.env.MQ_EARLY === '1';
const timers = new Map();
let maxMs = 0;
let core;

const fail = (what) => {
  console.log('FAIL ' + process.version + (early ? ' early' : '') + ': ' + what);
  process.exit(1);
};

globalThis.mqHost = {
  setTimer(ms, id) {
    if (timers.has(id)) fail('timer id ' + id + ' armed twice');
    maxMs = Math.max(maxMs, ms);
    // Node caps a delay at 2^31 - 1 ms and fires a longer one at once.
    const delay = early ? Math.floor(ms / 2) : ms;
    timers.set(id, setTimeout(() => {
      timers.delete(id);
      core.fire(id);
    }, delay));
  },
  clearTimer(id) {
    clearTimeout(timers.get(id));
    timers.delete(id);
  },
  log(level, line) {
    console.log('[' + level + '] ' + line);
  },
  died(message) {
    fail('died: ' + message);
  },
};

process.on('unhandledRejection', (e) => fail('unhandled rejection: ' + ((e && e.stack) || e)));
core = require(require('path').resolve(process.argv[2]));
core.selftest().then((passed) => {
  if (timers.size !== 0) fail(timers.size + ' timers left armed');
  if (maxMs !== 2147483647) fail('longest delay asked ' + maxMs + ', want 2147483647');
  console.log('PASS ' + process.version + (early ? ' early' : '') + ': ' + passed);
}, (e) => fail(String((e && e.stack) || e)));
