'use strict';
// The service's parent against a fake core: Host, Origin and token checks, the
// keystore, restarts, core.pid, keep-alive. Run by run.sh under Node 8.12 and 20.
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const h = require('./harness.js');
const { assert, test, sleep, until, alive } = h;

const closedWith = (c) => until('socket close', () => c.closed).then((x) => x.code);

test('wrong Host is refused over http and ws', async () => {
  const svc = await h.startService();
  assert.strictEqual((await h.get(svc, '/', { Host: 'evil.example:' + svc.port })).status, 421);
  assert.strictEqual((await h.get(svc, '/', { Host: '127.0.0.1' })).status, 421);
  assert.strictEqual((await h.get(svc, '/')).status, 200);
  await h.rejects(h.connect(svc, { host: 'evil.example:' + svc.port }).opened);
});

test('wrong or missing Origin is refused at the ws upgrade', async () => {
  const svc = await h.startService();
  await h.rejects(h.connect(svc, { origin: 'http://evil.example' }).opened);
  await h.rejects(h.connect(svc, { origin: '' }).opened);
  await (await h.authed(svc)).opened;
});

test('a bad token closes the socket with 4401', async () => {
  const svc = await h.startService();
  const c = h.connect(svc);
  await c.opened;
  c.send('not-the-token');
  assert.strictEqual(await closedWith(c), 4401);
  const longer = h.connect(svc);
  await longer.opened;
  longer.send(svc.token + 'x');
  assert.strictEqual(await closedWith(longer), 4401);
});

test('a good token, then text echoes through the child', async () => {
  const svc = await h.startService();
  const c = await h.authed(svc);
  c.send('hello');
  assert.strictEqual(await c.next(), 'echo:hello');
  assert.strictEqual(c.closed, null);
});

test('start refuses a sender that is not the app', async () => {
  const svc = await h.startService({ env: { FAKE_SENDER: 'app.other' } });
  assert.strictEqual(svc.start.returnValue, false);
  assert.strictEqual(svc.start.token, undefined);
  await sleep(200);
  assert.strictEqual(svc.find('listen').length, 0);
});

test('start answers with a token and the data dir probe', async () => {
  const svc = await h.startService();
  assert.strictEqual(svc.start.returnValue, true);
  assert.ok(/^[0-9a-f]{64}$/.test(svc.token));
  assert.strictEqual(svc.start.probe.ok, true);
});

test('keystore: write then read round-trips and survives a restart', async () => {
  const svc = await h.startService();
  const c = await h.authed(svc);
  c.send('{"host":"keystore","id":1,"op":"write","key":"debrid","value":{"records":[1],"tombstones":[]}}');
  assert.deepStrictEqual(JSON.parse(await c.next()), { host: 'keystore', id: 1, ok: true });
  c.send('{"host":"keystore","id":2,"op":"write","key":"trakt:p1","value":"tok"}');
  assert.strictEqual(JSON.parse(await c.next()).ok, true);
  c.send('{"host":"keystore","id":3,"op":"read"}');
  const read = JSON.parse(await c.next());
  assert.deepStrictEqual(read.value, { debrid: { records: [1], tombstones: [] }, 'trakt:p1': 'tok' });
  c.send('{"host":"keystore","id":4,"op":"write","key":"trakt:p1","value":null}');
  assert.strictEqual(JSON.parse(await c.next()).ok, true);
  await svc.kill();

  const again = await h.startService({ dir: svc.dir });
  assert.notStrictEqual(again.token, svc.token);
  const d = await h.authed(again);
  d.send('{"host":"keystore","id":9,"op":"read"}');
  assert.deepStrictEqual(JSON.parse(await d.next()).value, { debrid: { records: [1], tombstones: [] } });
});

test('keystore: a bad request is answered ok:false', async () => {
  const svc = await h.startService();
  const c = await h.authed(svc);
  c.send('{"host":"keystore","id":5,"op":"write"}');
  const r = JSON.parse(await c.next());
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.id, 5);
});

test('keystore before auth is refused and never answered', async () => {
  const svc = await h.startService();
  const c = h.connect(svc);
  await c.opened;
  c.send('{"host":"keystore","id":1,"op":"write","key":"debrid","value":"stolen"}');
  assert.strictEqual(await closedWith(c), 4401);
  assert.deepStrictEqual(c.messages, []);
  await sleep(100);
  assert.ok(!fs.existsSync(path.join(svc.data, 'secrets.json')));
});

test('keystore text never reaches the child', async () => {
  const svc = await h.startService();
  const c = await h.authed(svc);
  c.send('{"host":"keystore","id":1,"op":"read"}');
  c.send('after');
  await until('both answers', () => c.messages.length >= 2);
  await sleep(200);
  assert.strictEqual(c.messages.length, 2);
  assert.ok(c.messages.indexOf('echo:after') !== -1);
  assert.ok(!c.messages.some((m) => m.indexOf('echo:{') === 0));
});

test('the child dying restarts it, and text sent before ready is delivered after', async () => {
  const svc = await h.startService({ env: { FAKE_SLOW_MS: '600' } });
  const first = await h.corePid(svc);
  first.c.send('die');
  assert.strictEqual(await closedWith(first.c), 1012);
  // The restarted child takes 600 ms to open: this text waits for `ready`.
  const c = await h.authed(svc);
  c.send('queued');
  assert.strictEqual(svc.find('ready').length, 1);
  assert.strictEqual(await c.next('queued echo'), 'echo:queued');
  assert.strictEqual(svc.find('ready').length, 2);
  c.send('pid');
  assert.notStrictEqual(JSON.parse(await c.next()).pid, first.pid);
  assert.ok(!alive(first.pid));
});

test('an unhandled rejection in the child is a death, and the child is replaced', async () => {
  const svc = await h.startService();
  const first = await h.corePid(svc);
  first.c.send('reject');
  assert.strictEqual(await closedWith(first.c), 1012);
  assert.ok(svc.find('retire').some((e) => /died: unhandled rejection/.test(e.retire)));
  const second = await h.corePid(svc);
  assert.notStrictEqual(second.pid, first.pid);
});

test('a killed child restarts', async () => {
  const svc = await h.startService();
  const first = await h.corePid(svc);
  process.kill(first.pid, 'SIGKILL');
  assert.strictEqual(await closedWith(first.c), 1012);
  assert.notStrictEqual((await h.corePid(svc)).pid, first.pid);
});

test('a child that reports died but ignores SIGTERM is killed after 2 s, then replaced', async () => {
  const svc = await h.startService();
  const first = await h.corePid(svc);
  const t0 = Date.now();
  first.c.send('stuckdie');
  await closedWith(first.c);
  await until('child gone', () => !alive(first.pid), 6000);
  assert.ok(Date.now() - t0 >= 1900, 'SIGKILL came before the 2 s grace');
  assert.notStrictEqual((await h.corePid(svc)).pid, first.pid);
});

test('at most 3 restarts in 60 s: the 4th death stops the service', async () => {
  const svc = await h.startService();
  for (let i = 0; i < 4; i += 1) {
    const { c } = await h.corePid(svc);
    c.send('die');
    await closedWith(c);
  }
  await until('service stop', () => svc.exit, 5000);
  assert.strictEqual(svc.exit.code, 1);
  assert.strictEqual(svc.find('stop')[0].stop, 'restart cap');
  assert.strictEqual(svc.find('ready').length, 4);
});

test('the child exits when the parent is killed', async () => {
  const svc = await h.startService();
  const { pid } = await h.corePid(svc);
  await svc.kill();
  await until('child exit', () => !alive(pid), 5000);
});

const WORKER = path.join(__dirname, '..', 'core-worker.js');
const decoy = (args, ignoreTerm) => {
  const body = (ignoreTerm ? 'process.on("SIGTERM",function(){});' : '') + 'setInterval(function(){},1000)';
  return h.track(spawn(process.execPath, ['-e', body].concat(args), { stdio: 'ignore' }));
};

// A service dir with a decoy recorded in core.pid; `args(data)` is the decoy's command line tail.
async function withStale(args, ignoreTerm, env, preload) {
  const dir = h.tmpDir();
  const data = path.join(dir, 'data');
  fs.mkdirSync(data);
  const stale = decoy(args(data), ignoreTerm);
  await sleep(300);
  fs.writeFileSync(path.join(data, 'core.pid'), String(stale.pid));
  const svc = await h.startService({ dir, env: Object.assign({ EPERM_PID: String(stale.pid) }, env), preload });
  return { svc, stale, data };
}

test('a stale core.pid running core-worker.js on this data dir is killed before the first fork', async () => {
  const { svc, stale, data } = await withStale((d) => [WORKER, d]);
  await svc.waitFor('ready');
  assert.ok(!alive(stale.pid), 'stale core-worker still alive');
  assert.strictEqual(svc.find('stale')[0].stale, stale.pid);
  const { pid } = await h.corePid(svc);
  assert.strictEqual(fs.readFileSync(path.join(data, 'core.pid'), 'utf8'), String(pid));
});

test('a stale core.pid that ignores SIGTERM is killed after 2 s', async () => {
  const { svc, stale } = await withStale((d) => [WORKER, d], true);
  await svc.waitFor('ready');
  assert.ok(!alive(stale.pid));
});

test('a core.pid that does not hold core-worker is left alone', async () => {
  const { svc, stale } = await withStale(() => ['something-else']);
  await svc.waitFor('ready');
  assert.ok(alive(stale.pid), 'an unrelated process was killed');
  assert.strictEqual(svc.find('stale').length, 0);
  stale.kill('SIGKILL');
});

test('a core.pid holding core-worker.js of another data dir or another script is left alone', async () => {
  const a = await withStale(() => [WORKER, '/somewhere/else']);
  await a.svc.waitFor('ready');
  assert.ok(alive(a.stale.pid), 'another install data dir core was killed');
  a.stale.kill('SIGKILL');
  const b = await withStale((d) => ['/other/core-worker.js', d]);
  await b.svc.waitFor('ready');
  assert.ok(alive(b.stale.pid), 'another script was killed');
  b.stale.kill('SIGKILL');
});

test('a stale pid owned by another user (EPERM) does not crash the start', async () => {
  const { svc, stale } = await withStale((d) => [WORKER, d], false, {}, 'preload-eperm.js');
  await svc.waitFor('ready');
  assert.strictEqual(svc.exit, null);
  assert.ok(alive(stale.pid));
  stale.kill('SIGKILL');
});

test('a core.pid that cannot be written does not crash the start', async () => {
  const dir = h.tmpDir();
  fs.mkdirSync(path.join(dir, 'data'));
  fs.mkdirSync(path.join(dir, 'data', 'core.pid'));
  const svc = await h.startService({ dir });
  await svc.waitFor('ready');
  assert.strictEqual(svc.exit, null);
});

test('keep-alive: held at start, completed after the grace if no socket authenticates', async () => {
  const svc = await h.startService({ env: { MQ_GRACE_MS: '300' } });
  await svc.waitFor('activity');
  assert.strictEqual(svc.find('activity')[0].activity, 'create');
  await until('complete', () => svc.find('activity').length >= 2, 3000);
  assert.strictEqual(svc.find('activity')[1].activity, 'complete');
});

test('keep-alive: held while a socket is open, completed after the grace once it closes', async () => {
  const svc = await h.startService({ env: { MQ_GRACE_MS: '400' } });
  const c = await h.authed(svc);
  await sleep(900);
  assert.strictEqual(svc.find('activity').length, 1);
  c.ws.close();
  await until('complete', () => svc.find('activity').length >= 2, 3000);
  assert.strictEqual(svc.find('activity')[1].activity, 'complete');
});

test('/sub/ is served by the child, 404 on null; the UI falls back to index.html', async () => {
  const svc = await h.startService();
  await svc.waitFor('ready');
  const sub = await h.get(svc, '/sub/x.vtt');
  assert.strictEqual(sub.status, 200);
  assert.strictEqual(sub.body, 'WEBVTT\n');
  assert.strictEqual(sub.headers['content-type'], 'text/vtt');
  assert.strictEqual(sub.headers['cache-control'], 'no-store');
  assert.strictEqual((await h.get(svc, '/sub/none.vtt')).status, 404);
  const route = await h.get(svc, '/title/tt1');
  assert.strictEqual(route.status, 200);
  assert.ok(route.body.indexOf('<title>ui</title>') !== -1);
  assert.strictEqual(route.headers['content-security-policy'], "frame-ancestors 'none'");
  assert.strictEqual((await h.get(svc, '/app.js')).headers['content-type'], 'text/javascript; charset=utf-8');
  assert.strictEqual((await h.get(svc, '/missing.js')).status, 404);
  assert.notStrictEqual((await h.get(svc, '/..%2f..%2fdata%2fsecrets.json')).status, 200);
  assert.notStrictEqual((await h.get(svc, '/%2e%2e/data/core.pid')).status, 200);
});

test('GET /%00 is a 400 and the service stays up', async () => {
  const svc = await h.startService();
  await svc.waitFor('ready');
  assert.strictEqual((await h.get(svc, '/%00')).status, 400);
  assert.strictEqual((await h.get(svc, '/app%00.js')).status, 400);
  assert.strictEqual((await h.get(svc, '/')).status, 200);
  assert.strictEqual(svc.exit, null);
});

test('a text bigger than the IPC buffer, sent while the core is busy, is delivered once and not refused', async () => {
  const svc = await h.startService();
  const { c } = await h.corePid(svc);
  c.send('busy');
  for (let i = 0; i < 20; i += 1) c.send('x'.repeat(300000));
  await until('all big answers', () => c.messages.length >= 20, 20000);
  assert.strictEqual(c.closed, null);
  assert.ok(c.messages.every((m) => m === 'big:300000'));
  await sleep(300);
  assert.strictEqual(c.messages.length, 20);
});

test('more than 8 MB in flight to a wedged core closes the socket with 1013', async () => {
  const svc = await h.startService();
  const { c } = await h.corePid(svc);
  c.send('busy');
  for (let i = 0; i < 40; i += 1) c.send('x'.repeat(300000));
  assert.strictEqual(await closedWith(c), 1013);
  assert.strictEqual(svc.exit, null);
  // The core catches up and takes the next socket's traffic.
  const d = await h.authed(svc);
  d.send('after');
  await until('echo after the backlog', () => d.messages.indexOf('echo:after') !== -1, 15000);
});

test('a pre-auth message over 256 bytes is refused with 1009 before it is buffered; over 64 is a bad token', async () => {
  const svc = await h.startService();
  const c = h.connect(svc);
  await c.opened;
  c.send('x'.repeat(2000000));
  assert.strictEqual(await closedWith(c), 1009);
  const mid = h.connect(svc);
  await mid.opened;
  mid.send('x'.repeat(200));
  assert.strictEqual(await closedWith(mid), 4401);
  // After auth the same size is fine.
  const d = await h.authed(svc);
  d.send('y'.repeat(300000));
  assert.strictEqual(await d.next(), 'big:300000');
});

test('a failed listen (EADDRINUSE) answers false and still releases the keep-alive', async () => {
  const port = await h.freePort();
  const blocker = require('net').createServer().listen(port, '127.0.0.1');
  await new Promise((r) => blocker.on('listening', r));
  const svc = await h.startService({ port, env: { MQ_GRACE_MS: '300' } });
  assert.strictEqual(svc.start.returnValue, false);
  assert.strictEqual(svc.start.errorText, 'EADDRINUSE');
  await until('keep-alive released', () => svc.find('activity').length >= 2, 3000);
  assert.deepStrictEqual(svc.find('activity').map((e) => e.activity), ['create', 'complete']);
  blocker.close();
});

test('errors on the http server after listen are logged, not fatal', async () => {
  const svc = await h.startService({ preload: 'preload-server-error.js' });
  await svc.waitFor('ready');
  await sleep(900);
  assert.strictEqual(svc.exit, null);
  assert.strictEqual((await h.get(svc, '/')).status, 200);
  assert.strictEqual(svc.find('serverError').length, 2);
});

test('/sub/ answers from the core are sandboxed; a bad status or header is a 502', async () => {
  const svc = await h.startService();
  await svc.waitFor('ready');
  const ok = await h.get(svc, '/sub/x.vtt');
  assert.strictEqual(ok.status, 200);
  assert.strictEqual(ok.headers['x-content-type-options'], 'nosniff');
  assert.strictEqual(ok.headers['content-security-policy'], 'sandbox');
  assert.strictEqual((await h.get(svc, '/sub/status')).status, 502);
  assert.strictEqual((await h.get(svc, '/sub/header')).status, 502);
  assert.strictEqual(svc.exit, null);
  assert.strictEqual((await h.get(svc, '/sub/x.vtt')).status, 200);
});

test('device-seed: a bad file is replaced by 64 hex chars, a good one is kept', async () => {
  const dir = h.tmpDir();
  fs.mkdirSync(path.join(dir, 'data'));
  const file = path.join(dir, 'data', 'device-seed');
  fs.writeFileSync(file, 'zz not a seed');
  const svc = await h.startService({ dir });
  const { c } = await h.corePid(svc);
  c.send('config');
  const seed = JSON.parse(await c.next()).deviceSeed;
  assert.ok(/^[0-9a-f]{64}$/.test(seed), 'seed was ' + seed);
  assert.strictEqual(fs.readFileSync(file, 'utf8'), seed);
  await svc.kill();
  const again = await h.startService({ dir });
  const d = await h.authed(again);
  d.send('config');
  assert.strictEqual(JSON.parse(await d.next()).deviceSeed, seed);
});

h.run();
