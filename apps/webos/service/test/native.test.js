'use strict';
// The native core (plans/webos.md v5) against a fake bridge (fake-bridge.js):
// it starts with its token and data dir, each page socket gets a session of
// its own there, and a binary that cannot start or keeps dying gives way to
// the wasm core. Run by run.sh under Node 8.12 and 20.
const fs = require('fs');
const path = require('path');
const h = require('./harness.js');
const { assert, test, until, alive } = h;

const FAKE = path.join(__dirname, 'fake-bridge.js');
const NATIVE = { query: '?core=native' };
const closedWith = (c) => until('socket close', () => c.closed).then((x) => x.code);
const start = (env) => h.startService({ env: Object.assign({ MQ_NATIVE: FAKE, MQ_NATIVE_QUIET_MS: '3000' }, env) });
const coreOf = (svc) => svc.find('ready').map((e) => e.core);

test('the native core starts with its own token, a free port, the wasm core\'s device id and its data dir', async () => {
  const svc = await start();
  await svc.waitFor('ready');
  assert.deepStrictEqual(coreOf(svc), ['native']);
  const c = await h.authed(svc, NATIVE);
  c.send('hello');
  assert.strictEqual(await c.next(), 'native:hello');
  c.send('env');
  const env = JSON.parse(await c.next());
  assert.deepStrictEqual(env.argv, [svc.data]);
  assert.strictEqual(env.data, path.join(svc.data, 'native'));
  assert.strictEqual(env.port, '0');
  assert.ok(env.bound > 0);
  assert.strictEqual(svc.find('ready')[0].open.port, env.bound);
  assert.ok(/^[0-9a-f]{64}$/.test(env.token) && env.token !== svc.token, 'its own token, not the page one');
  assert.strictEqual(env.device, fs.readFileSync(path.join(svc.data, 'device-seed'), 'utf8').slice(0, 16));
  assert.strictEqual(env.heartbeat, '2000');
});

test('the port after the page\'s, taken by another program, does not stop the native core', async () => {
  const port = await h.freePort();
  const squatter = require('net').createServer();
  await new Promise((r) => squatter.listen(port + 1, '127.0.0.1', r));
  try {
    const svc = await h.startService({ port, env: { MQ_NATIVE: FAKE, MQ_NATIVE_QUIET_MS: '3000' } });
    await svc.waitFor('ready');
    assert.deepStrictEqual(coreOf(svc), ['native']);
    const c = await h.authed(svc, NATIVE);
    c.send('x');
    assert.strictEqual(await c.next(), 'native:x');
  } finally {
    squatter.close();
  }
});

test('a slow first open that keeps its heartbeat is waited for past the quiet limit', async () => {
  const svc = await start({ FAKE_BRIDGE: 'slow', FAKE_OPEN_MS: '1500', FAKE_BEAT_MS: '200', MQ_NATIVE_QUIET_MS: '600' });
  await svc.waitFor('ready');
  assert.deepStrictEqual(coreOf(svc), ['native']);
});

test('a heartbeat with no ready line stops at the hard cap, and the reason is logged', async () => {
  const svc = await start({ FAKE_BRIDGE: 'hang', FAKE_BEAT_MS: '200', MQ_NATIVE_QUIET_MS: '600', MQ_NATIVE_CAP_MS: '1500' });
  await svc.waitFor('ready');
  assert.deepStrictEqual(coreOf(svc), ['wasm']);
  const off = svc.find('native')[0];
  assert.strictEqual(off.why, 'never ready');
  assert.strictEqual(off.detail, 'not ready in 1500 ms');
});

test('index.html carries the native mark only while the native core runs', async () => {
  const svc = await start();
  await svc.waitFor('ready');
  const page = await h.get(svc, '/');
  assert.ok(page.body.indexOf('<!doctype html><meta name="mq-core" content="native"><title>ui</title>') === 0, page.body);
  const wasm = await h.startService();
  assert.strictEqual((await h.get(wasm, '/')).body.indexOf('mq-core'), -1);
});

test('a page built for the other core is closed with 1012 after its token, so it reloads', async () => {
  const svc = await start();
  await svc.waitFor('ready');
  const old = await h.authed(svc);
  assert.strictEqual(await closedWith(old), 1012);
  const wasm = await h.startService();
  const stale = await h.authed(wasm, NATIVE);
  assert.strictEqual(await closedWith(stale), 1012);
  const fine = await h.authed(wasm, { query: '?core=wasm' });
  fine.send('x');
  assert.strictEqual(await fine.next(), 'echo:x');
});

test('each page socket has a session of its own in the bridge', async () => {
  const svc = await start();
  const a = await h.authed(svc, NATIVE);
  a.send('n');
  const first = await a.next();
  const b = await h.authed(svc, NATIVE);
  b.send('n');
  const second = await b.next();
  assert.notStrictEqual(first, second);
  assert.strictEqual(a.messages.length, 0);
});

test('a closed page socket ends its session in the bridge', async () => {
  const svc = await start();
  const a = await h.authed(svc, NATIVE);
  a.send('n');
  await a.next();
  const b = await h.authed(svc, NATIVE);
  b.send('open');
  assert.strictEqual(await b.next(), 'open:2');
  a.ws.close();
  await until('the bridge session closed', async () => {
    b.send('open');
    return (await b.next()) === 'open:1';
  });
});

test('texts of several MB go through the bridge whole, both ways', async () => {
  const svc = await start();
  const c = await h.authed(svc, NATIVE);
  c.send('y'.repeat(3 * 1024 * 1024));
  assert.strictEqual(await c.next(), 'got:' + 3 * 1024 * 1024);
  c.send('big:' + 5 * 1024 * 1024);
  assert.strictEqual((await c.next()).length, 5 * 1024 * 1024);
  assert.strictEqual(c.closed, null);
});

// A wasm library as the v0.3.0 IPK leaves it (fake bytes: the service copies files, it never reads them).
function wasmLibrary(data, journal) {
  fs.writeFileSync(path.join(data, 'library.sqlite'), 'SQLite format 3\u0000 the wasm library');
  if (journal !== undefined) fs.writeFileSync(path.join(data, 'library.sqlite-journal'), journal);
}

test('the first native start takes the wasm core\'s library, once', async () => {
  const dir = h.tmpDir();
  const data = path.join(dir, 'data');
  fs.mkdirSync(data);
  wasmLibrary(data, '');
  const svc = await h.startService({ dir, env: { MQ_NATIVE: FAKE } });
  await svc.waitFor('ready');
  const copied = path.join(data, 'native', 'library.sqlite');
  assert.strictEqual(fs.readFileSync(copied, 'utf8'), fs.readFileSync(path.join(data, 'library.sqlite'), 'utf8'));
  // A finished commit's empty journal stays behind; no temp file is left.
  assert.ok(!fs.existsSync(copied + '-journal'));
  assert.deepStrictEqual(fs.readdirSync(path.join(data, 'native')).filter((f) => /\.tmp$/.test(f)), []);
  assert.deepStrictEqual(svc.find('adopted')[0].adopted, { bytes: fs.statSync(copied).size, journal: false });
  await svc.kill();
  // The native library is its own from then on: a later start never copies again.
  fs.writeFileSync(copied, 'the native library');
  const again = await h.startService({ dir, env: { MQ_NATIVE: FAKE } });
  await again.waitFor('ready');
  assert.strictEqual(fs.readFileSync(copied, 'utf8'), 'the native library');
  assert.strictEqual(again.find('adopted').length, 0);
});

test('a wasm library cut mid-transaction goes with its journal, so the first open rolls it back', async () => {
  const dir = h.tmpDir();
  const data = path.join(dir, 'data');
  fs.mkdirSync(data);
  wasmLibrary(data, 'a hot journal');
  const svc = await h.startService({ dir, env: { MQ_NATIVE: FAKE } });
  await svc.waitFor('ready');
  assert.strictEqual(fs.readFileSync(path.join(data, 'native', 'library.sqlite-journal'), 'utf8'), 'a hot journal');
  assert.strictEqual(svc.find('adopted')[0].adopted.journal, true);
});

test('no wasm library, or the wasm core alone: nothing is copied', async () => {
  const fresh = await start();
  await fresh.waitFor('ready');
  assert.strictEqual(fresh.find('adopted').length, 0);
  const dir = h.tmpDir();
  const data = path.join(dir, 'data');
  fs.mkdirSync(data);
  wasmLibrary(data);
  const wasm = await h.startService({ dir });
  await wasm.waitFor('ready');
  assert.ok(!fs.existsSync(path.join(data, 'native')));
});

test('/sub/ is no route with the native core: its stream server serves subtitles', async () => {
  const svc = await start();
  await svc.waitFor('ready');
  assert.strictEqual((await h.get(svc, '/sub/x.vtt')).status, 404);
});

test('a native core that never says ready is killed and the wasm core runs', async () => {
  const svc = await start({ FAKE_BRIDGE: 'silent', MQ_NATIVE_QUIET_MS: '500' });
  await svc.waitFor('ready');
  assert.deepStrictEqual(coreOf(svc), ['wasm']);
  assert.strictEqual(svc.find('native')[0].why, 'never ready');
  assert.strictEqual(svc.find('native')[0].detail, 'no line in 500 ms');
  assert.strictEqual(svc.find('retire').length, 1, 'no second try of a core that never started');
  assert.strictEqual((await h.get(svc, '/')).body.indexOf('mq-core'), -1);
  const c = await h.authed(svc, { query: '?core=wasm' });
  c.send('x');
  assert.strictEqual(await c.next(), 'echo:x');
});

test('a native core that exits at start, or a file that cannot run, gives the wasm core', async () => {
  const exits = await start({ FAKE_BRIDGE: 'exit' });
  await exits.waitFor('ready');
  assert.deepStrictEqual(coreOf(exits), ['wasm']);
  assert.strictEqual(exits.find('native')[0].detail, 'exit 2');
  assert.strictEqual(exits.find('retire').length, 1);
  // Not a program (no ELF, no #!): exec refuses it, as a TV refuses another arch.
  const junk = path.join(h.tmpDir(), 'mq-bridge');
  fs.writeFileSync(junk, 'not a program\n');
  const refused = await h.startService({ env: { MQ_NATIVE: junk } });
  await refused.waitFor('ready');
  assert.deepStrictEqual(coreOf(refused), ['wasm']);
  assert.strictEqual(refused.find('native')[0].why, 'never ready');
  const missing = await h.startService({ env: { MQ_NATIVE: junk + '-none' } });
  await missing.waitFor('ready');
  assert.deepStrictEqual(coreOf(missing), ['wasm']);
});

test('a native core that keeps dying: restarted twice, then the wasm core, and the page reloads', async () => {
  const svc = await start();
  for (let i = 0; i < 4; i += 1) {
    await svc.waitFor('ready', i + 1);
    const c = await h.authed(svc, NATIVE);
    c.send('die');
    assert.strictEqual(await closedWith(c), 1012);
  }
  await svc.waitFor('ready', 5);
  assert.deepStrictEqual(coreOf(svc), ['native', 'native', 'native', 'native', 'wasm']);
  assert.strictEqual(svc.find('native')[0].why, 'restart cap');
  assert.strictEqual(svc.exit, null, 'the service keeps running');
  const c = await h.authed(svc, { query: '?core=wasm' });
  c.send('x');
  assert.strictEqual(await c.next(), 'echo:x');
});

test('the bridge exits when the service is killed (its stdin closes)', async () => {
  const svc = await start();
  const c = await h.authed(svc, NATIVE);
  c.send('pid');
  const pid = JSON.parse(await c.next()).pid;
  assert.ok(alive(pid));
  await svc.kill();
  await until('bridge exit', () => !alive(pid), 5000);
});

test('a stale core.pid holding this install\'s bridge on this data dir is killed first', async () => {
  const first = await start({ FAKE_BRIDGE: 'orphan' });
  const c = await h.authed(first, NATIVE);
  c.send('pid');
  const pid = JSON.parse(await c.next()).pid;
  assert.strictEqual(fs.readFileSync(path.join(first.data, 'core.pid'), 'utf8'), String(pid));
  // A bridge that outlived its killed service: the next start must not run a
  // second core over the same files.
  await first.kill();
  assert.ok(alive(pid));
  // A port of its own: the old bridge took a free one, which may sit right after the old page's.
  const second = await h.startService({ dir: first.dir, port: await h.freePort(), env: { MQ_NATIVE: FAKE, MQ_NATIVE_QUIET_MS: '3000' } });
  await until('stale bridge killed', () => !alive(pid), 8000);
  assert.ok(second.find('stale').some((e) => e.stale === pid));
  await second.waitFor('ready');
  assert.deepStrictEqual(coreOf(second), ['native']);
});

h.run();
