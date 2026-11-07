'use strict';
// keystore.js on its own: ordering, deletes, a failed write, a corrupt file.
const fs = require('fs');
const path = require('path');
const h = require('./harness.js');
const keystore = require('../keystore.js');
const { assert, test } = h;

const ask = (ks, o) => ks.handle(JSON.stringify(Object.assign({ host: 'keystore' }, o))).then(JSON.parse);

test('writes apply in arrival order and the file holds the last one', async () => {
  const dir = h.tmpDir();
  const ks = keystore.create(dir);
  const replies = await Promise.all([1, 2, 3, 4, 5].map((n) => ask(ks, { id: n, op: 'write', key: 'sync', value: n })));
  assert.deepStrictEqual(replies.map((r) => r.id), [1, 2, 3, 4, 5]);
  assert.ok(replies.every((r) => r.ok));
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(path.join(dir, 'secrets.json'), 'utf8')), { sync: 5 });
  assert.ok(!fs.existsSync(path.join(dir, 'secrets.json.tmp')));
});

test('a failed write keeps the old store and answers ok:false', async () => {
  const dir = h.tmpDir();
  const ks = keystore.create(dir);
  await ask(ks, { id: 1, op: 'write', key: 'a', value: 1 });
  // A directory in the temp file's place makes the next write fail.
  fs.mkdirSync(path.join(dir, 'secrets.json.tmp'));
  const bad = await ask(ks, { id: 2, op: 'write', key: 'b', value: 2 });
  assert.strictEqual(bad.ok, false);
  assert.deepStrictEqual((await ask(ks, { id: 3, op: 'read' })).value, { a: 1 });
  fs.rmdirSync(path.join(dir, 'secrets.json.tmp'));
  assert.strictEqual((await ask(ks, { id: 4, op: 'write', key: 'b', value: 2 })).ok, true);
});

test('bad input is answered, never thrown', async () => {
  const ks = keystore.create(h.tmpDir());
  assert.strictEqual(JSON.parse(await ks.handle('{"host":"keystore"')).ok, false);
  assert.strictEqual(JSON.parse(await ks.handle('null')).ok, false);
  assert.strictEqual((await ask(ks, { id: 1, op: 'frobnicate' })).ok, false);
  assert.strictEqual((await ask(ks, { id: 2, op: 'write', key: '', value: 1 })).ok, false);
});

test('a corrupt secrets.json is kept aside and the store starts empty', async () => {
  const dir = h.tmpDir();
  fs.writeFileSync(path.join(dir, 'secrets.json'), '{not json');
  const ks = keystore.create(dir);
  assert.deepStrictEqual((await ask(ks, { id: 1, op: 'read' })).value, {});
  assert.strictEqual(fs.readFileSync(path.join(dir, 'secrets.json.corrupt'), 'utf8'), '{not json');
});

test('a short write is finished: every byte reaches the file', async () => {
  const dir = h.tmpDir();
  const ks = keystore.create(dir);
  const write = fs.writeSync;
  // Every call writes at most 3 bytes, a string or a buffer, as a full disk or a pipe can.
  fs.writeSync = function (fd, data, off, len) {
    if (typeof data === 'string') return write.call(fs, fd, data.slice(0, 3));
    return write.call(fs, fd, data, off, Math.min(len, 3));
  };
  try {
    const big = 'v'.repeat(500);
    assert.strictEqual((await ask(ks, { id: 1, op: 'write', key: 'a', value: big })).ok, true);
  } finally {
    fs.writeSync = write;
  }
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(path.join(dir, 'secrets.json'), 'utf8')), { a: 'v'.repeat(500) });
});

test('an unreadable secrets.json is never moved aside or overwritten, and is read again later', async () => {
  const dir = h.tmpDir();
  // A directory in the file's place: a read error that is not a syntax error.
  fs.mkdirSync(path.join(dir, 'secrets.json'));
  const ks = keystore.create(dir);
  assert.strictEqual((await ask(ks, { id: 1, op: 'read' })).ok, false);
  assert.strictEqual((await ask(ks, { id: 2, op: 'write', key: 'a', value: 1 })).ok, false);
  assert.ok(fs.statSync(path.join(dir, 'secrets.json')).isDirectory());
  assert.ok(!fs.existsSync(path.join(dir, 'secrets.json.corrupt')));
  fs.rmdirSync(path.join(dir, 'secrets.json'));
  fs.writeFileSync(path.join(dir, 'secrets.json'), '{"kept":true}');
  assert.deepStrictEqual((await ask(ks, { id: 3, op: 'read' })).value, { kept: true });
});

test('a secrets.json that is valid JSON but not an object counts as corrupt', async () => {
  const dir = h.tmpDir();
  fs.writeFileSync(path.join(dir, 'secrets.json'), '[1,2]');
  const ks = keystore.create(dir);
  assert.deepStrictEqual((await ask(ks, { id: 1, op: 'read' })).value, {});
  assert.strictEqual(fs.readFileSync(path.join(dir, 'secrets.json.corrupt'), 'utf8'), '[1,2]');
});

h.run();
