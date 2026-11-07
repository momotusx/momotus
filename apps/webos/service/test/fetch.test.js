'use strict';
// host.js `fetch` (plans/webos.md 3.2) against a local server: gzip decoded,
// the cap on decoded bytes, a prefix, address fallback, timeout and abort.
const http = require('http');
const zlib = require('zlib');
const h = require('./harness.js');
const host = require('../host.js');
const { assert, test, sleep, until } = h;

const answers = new Map();
const core = { fetched: (id, json) => answers.set(id, JSON.parse(json)) };
let seenHost = null;
let n = 0;

const server = http.createServer((req, res) => {
  seenHost = req.headers.host;
  if (req.url === '/gzip') {
    res.writeHead(200, { 'Content-Encoding': 'gzip', 'Content-Type': 'text/plain', 'X-A': '1' });
    return res.end(zlib.gzipSync(Buffer.alloc(5000, 'a')));
  }
  if (req.url === '/hang') return;
  res.writeHead(200, { 'Content-Type': 'text/plain' });
  res.end('0123456789');
});

async function call(extra) {
  const id = (n += 1);
  const req = Object.assign({ method: 'GET', headers: [], addresses: ['127.0.0.1'], timeoutMs: 2000, maxBytes: 1000, url: '' }, extra);
  global.mqHost.fetch(id, JSON.stringify(req));
  return { id, answer: () => until('fetched ' + id, () => answers.get(id)) };
}

const listening = new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const url = (p) => 'http://example.test:' + server.address().port + p;

test('fetch installs', async () => {
  await listening;
  host.install(() => core);
});

test('a gzip body is decoded, content-encoding dropped, Host is the name', async () => {
  const r = await (await call({ url: url('/gzip'), maxBytes: 10000 })).answer();
  assert.strictEqual(r.status, 200);
  assert.strictEqual(Buffer.from(r.body, 'base64').length, 5000);
  assert.ok(!r.headers.some((x) => /content-encoding/i.test(x[0])));
  assert.ok(r.headers.some((x) => x[0] === 'X-A' && x[1] === '1'));
  assert.strictEqual(seenHost, 'example.test:' + server.address().port);
});

test('the cap counts decoded bytes', async () => {
  const r = await (await call({ url: url('/gzip'), maxBytes: 4000 })).answer();
  assert.deepStrictEqual(r, { error: 'cap' });
});

test('a prefix is cut and flagged truncated', async () => {
  const r = await (await call({ url: url('/x'), prefixBytes: 4 })).answer();
  assert.strictEqual(Buffer.from(r.body, 'base64').toString(), '0123');
  assert.strictEqual(r.truncated, true);
});

test('the next address is tried when a connection is refused', async () => {
  const r = await (await call({ url: url('/x'), addresses: ['::1', '127.0.0.1'] })).answer();
  assert.strictEqual(Buffer.from(r.body, 'base64').toString(), '0123456789');
});

test('every address refused is `refused`; no addresses is `dns`', async () => {
  assert.deepStrictEqual(await (await call({ url: url('/x'), addresses: ['::1'] })).answer(), { error: 'refused' });
  assert.deepStrictEqual(await (await call({ url: url('/x'), addresses: [] })).answer(), { error: 'dns' });
});

test('a silent server times out', async () => {
  const r = await (await call({ url: url('/hang'), timeoutMs: 200 })).answer();
  assert.deepStrictEqual(r, { error: 'timeout' });
});

test('abort answers nothing', async () => {
  const c = await call({ url: url('/hang'), timeoutMs: 400 });
  await sleep(50);
  global.mqHost.abort(c.id);
  await sleep(700);
  assert.strictEqual(answers.has(c.id), false);
});

test('resolve answers addresses, IPv4 first', async () => {
  let got = null;
  core.resolved = (id, json) => (got = JSON.parse(json));
  global.mqHost.resolve(1, 'localhost');
  await until('resolved', () => got);
  assert.ok(got.length >= 1 && got.indexOf('127.0.0.1') === 0);
});

test('a malformed request is answered `reset`, never thrown into the core', async () => {
  const bad = [
    'not json',
    JSON.stringify({ method: 'GET', url: 'not a url', headers: [], addresses: ['127.0.0.1'], timeoutMs: 100, maxBytes: 10 }),
    JSON.stringify({ method: 'GET', url: url('/x'), headers: [], timeoutMs: 100, maxBytes: 10 }),
    JSON.stringify({ method: 'GET', url: url('/x'), headers: [['Bad\nName', 'x']], addresses: ['127.0.0.1'], timeoutMs: 100, maxBytes: 10 }),
    JSON.stringify({ method: 'GET', url: url('/x'), headers: [['X', 'a\r\nb']], addresses: ['127.0.0.1'], timeoutMs: 100, maxBytes: 10 }),
    'null',
  ];
  const ids = bad.map((request, i) => {
    const id = 1000 + i;
    global.mqHost.fetch(id, request);
    // The core must never be re-entered from inside the import that started it.
    assert.strictEqual(answers.has(id), false, 'case ' + i + ' was answered inside fetch');
    return id;
  });
  for (const id of ids) assert.deepStrictEqual(await until('answer ' + id, () => answers.get(id)), { error: 'reset' });
});

test('no-address and an abort right after an answer is queued: answered later, or never after abort', async () => {
  const dns = 2000;
  global.mqHost.fetch(dns, JSON.stringify({ method: 'GET', url: url('/x'), headers: [], addresses: [], timeoutMs: 100, maxBytes: 10 }));
  assert.strictEqual(answers.has(dns), false, 'dns answer came inside fetch');
  assert.deepStrictEqual(await until('dns answer', () => answers.get(dns)), { error: 'dns' });
  const gone = 2001;
  global.mqHost.fetch(gone, JSON.stringify({ method: 'GET', url: url('/x'), headers: [], addresses: [], timeoutMs: 100, maxBytes: 10 }));
  global.mqHost.abort(gone);
  await sleep(100);
  assert.strictEqual(answers.has(gone), false);
});

h.run();
