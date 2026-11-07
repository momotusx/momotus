'use strict';
// The keystore (plans/webos.md 4, wire v4.3): the whole store is one
// secrets.json under the data dir, written atomically (temp, fsync, rename,
// directory fsync) before any ok. Writes run one at a time in arrival order.
const fs = require('fs');
const path = require('path');

function fsyncPath(p, flags) {
  const fd = fs.openSync(p, flags);
  try {
    fs.fsyncSync(fd);
  } finally {
    fs.closeSync(fd);
  }
}

function writeAtomic(file, text) {
  const tmp = file + '.tmp';
  const fd = fs.openSync(tmp, 'w', 0o600);
  try {
    // writeSync may write less than asked: go on until every byte is down.
    const buf = Buffer.from(text);
    for (let put = 0; put < buf.length; ) put += fs.writeSync(fd, buf, put, buf.length - put);
    fs.fsyncSync(fd);
  } finally {
    fs.closeSync(fd);
  }
  fs.renameSync(tmp, file);
  fsyncPath(path.dirname(file), 'r');
}

// A missing file is an empty store; text that is not a JSON object is kept
// aside for the owner. Any other read error throws, and nothing is overwritten.
function load(file) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (e) {
    if (e.code === 'ENOENT') return {};
    throw e;
  }
  try {
    const store = JSON.parse(text);
    if (!store || typeof store !== 'object' || Array.isArray(store)) throw new SyntaxError('not an object');
    return store;
  } catch (e) {
    if (!(e instanceof SyntaxError)) throw e;
    fs.renameSync(file, file + '.corrupt');
    return {};
  }
}

// `handle(text)` answers one keystore message with the reply text.
function create(dir) {
  const file = path.join(dir, 'secrets.json');
  let store = null;
  let tail = Promise.resolve();
  const reply = (id, rest) => JSON.stringify(Object.assign({ host: 'keystore', id }, rest));

  function run(req) {
    // Read on the first request, and again after a failed read.
    if (!store) store = load(file);
    if (req.op === 'read') return reply(req.id, { ok: true, value: store });
    if (req.op !== 'write' || typeof req.key !== 'string' || !req.key) throw new Error('bad request');
    const next = Object.assign({}, store);
    if (req.value === null || req.value === undefined) delete next[req.key];
    else next[req.key] = req.value;
    writeAtomic(file, JSON.stringify(next));
    store = next;
    return reply(req.id, { ok: true });
  }

  function handle(text) {
    let req;
    try {
      req = JSON.parse(text);
    } catch (e) {
      return Promise.resolve(reply(null, { ok: false, error: 'bad json' }));
    }
    if (!req || typeof req !== 'object') return Promise.resolve(reply(null, { ok: false, error: 'bad json' }));
    const id = typeof req.id === 'number' ? req.id : null;
    const answer = tail.then(() => run(req)).catch((e) => reply(id, { ok: false, error: String((e && e.message) || e) }));
    tail = answer;
    return answer;
  }

  return { handle };
}

module.exports = { create, writeAtomic, PREFIX: '{"host":"keystore"' };
