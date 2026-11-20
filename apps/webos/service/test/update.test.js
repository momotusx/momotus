'use strict';
// update.js and update-install.js on their own (plans/auto-update.md section 2):
// the signed feed against the shared vectors, a real IPK checked and staged,
// refusals, and the detached install reading luna-send-pub's replies.
const fs = require('fs');
const http = require('http');
const path = require('path');
const zlib = require('zlib');
const { execFileSync, spawn } = require('child_process');
const h = require('./harness.js');
const update = require('../update.js');
const { assert, test } = h;

const APP = 'app.momotus.webos';
const vectors = JSON.parse(fs.readFileSync(path.join(__dirname, '../../../shared/fixtures/update-vectors.json'), 'utf8'));

// A ustar entry: 512-byte header, the body padded to 512.
function tarEntry(name, body) {
  const head = Buffer.alloc(512);
  head.write(name, 0, 'latin1');
  head.write('0000644\0', 100, 'latin1');
  head.write('0000000\0', 108, 'latin1');
  head.write('0000000\0', 116, 'latin1');
  head.write(('00000000000' + body.length.toString(8)).slice(-11) + '\0', 124, 'latin1');
  head.write('00000000000\0', 136, 'latin1');
  head.write('        ', 148, 'latin1');
  head.write('0', 156, 'latin1');
  let sum = 0;
  for (let i = 0; i < 512; i += 1) sum += head[i];
  head.write(('000000' + sum.toString(8)).slice(-6) + '\0 ', 148, 'latin1');
  const pad = Buffer.alloc((512 - (body.length % 512)) % 512);
  return Buffer.concat([head, body, pad]);
}

// An ar member: 60-byte header, the body padded to an even length.
function arMember(name, body) {
  const field = (s, n) => (s + ' '.repeat(n)).slice(0, n);
  const head = field(name, 16) + field('0', 12) + field('0', 6) + field('0', 6) + field('100644', 8) + field(String(body.length), 10) + '`\n';
  return Buffer.concat([Buffer.from(head, 'latin1'), body, Buffer.alloc(body.length % 2, '\n')]);
}

function ipk(pkg) {
  const control = Buffer.from('Package: ' + pkg + '\nVersion: 0.5.0\nArchitecture: all\n');
  const tar = Buffer.concat([tarEntry('./', Buffer.alloc(0)), tarEntry('./control', control), Buffer.alloc(1024)]);
  return Buffer.concat([
    Buffer.from('!<arch>\n', 'latin1'),
    arMember('debian-binary', Buffer.from('2.0\n')),
    arMember('control.tar.gz', zlib.gzipSync(tar)),
    arMember('data.tar.gz', zlib.gzipSync(Buffer.alloc(1024))),
  ]);
}

// A P-256 pair from openssl (Node 8 cannot make one), and a feed signed with it.
function keys(dir) {
  const priv = path.join(dir, 'key.pem');
  execFileSync('openssl', ['ecparam', '-name', 'prime256v1', '-genkey', '-noout', '-out', priv]);
  const pub = execFileSync('openssl', ['ec', '-in', priv, '-pubout'], { stdio: ['ignore', 'pipe', 'ignore'] }).toString();
  return { priv, pub };
}

function sign(key, body) {
  const signed = JSON.stringify(body);
  const sig = execFileSync('openssl', ['dgst', '-sha256', '-sign', key.priv], { input: signed }).toString('base64');
  return JSON.stringify({ signed, sig });
}

// A local server for the feed and the IPK; `get` maps the feed's https URLs onto it.
async function site(files) {
  const port = await h.freePort();
  const server = http.createServer((req, res) => {
    const body = files[req.url];
    if (req.url === '/moved') {
      res.writeHead(302, { location: '/ipk' });
      return res.end();
    }
    if (body === undefined) {
      res.writeHead(404);
      return res.end();
    }
    res.writeHead(200);
    res.end(body);
  });
  await new Promise((r) => server.listen(port, '127.0.0.1', r));
  const base = 'http://127.0.0.1:' + port;
  const get = (url, limit, sink) => update.get(url.replace(/^https:\/\/example\.test/, base), limit, sink);
  return { base, get, close: () => server.close() };
}

function entry(bytes, extra) {
  const sha256 = require('crypto').createHash('sha256').update(bytes).digest('hex');
  return Object.assign({ version: '0.5.0', url: 'https://example.test/moved', sha256, size: bytes.length }, extra);
}

function updater(o) {
  const states = [];
  const u = update.create(
    Object.assign(
      { app: APP, version: '0.4.0', dataDir: o.dir, tmp: o.dir, feed: 'https://example.test/feed', onState: (s) => states.push(s) },
      o,
    ),
  );
  return { u, states };
}

test('only envelopes signed by the key verify, as on Android (shared vectors)', async () => {
  for (const name of Object.keys(vectors.verifies)) {
    assert.strictEqual(update.verified(vectors.feeds[name], vectors.publicKey) !== null, vectors.verifies[name], name);
  }
});

test('the webos entry is offered only when newer and well formed', async () => {
  const body = update.verified(vectors.feeds.good, vectors.publicKey);
  assert.strictEqual(update.pick(body, '0.4.0').version, '0.5.0');
  assert.strictEqual(update.pick(body, '0.5.0'), null);
  assert.strictEqual(update.pick(body, '0.10.0'), null);
  assert.ok(update.newer('0.10.0', '0.9.9'));
  const good = body.webos;
  const bad = [{ url: 'http://x/y.ipk' }, { sha256: 'A'.repeat(64) }, { size: 0 }, { version: '1.0' }];
  for (const b of bad) assert.strictEqual(update.pick({ webos: Object.assign({}, good, b) }, '0.4.0'), null, JSON.stringify(b));
});

test('an IPK is read for its Package without ar or tar', async () => {
  const dir = h.tmpDir();
  fs.writeFileSync(path.join(dir, 'a.ipk'), ipk(APP));
  assert.strictEqual(update.ipkPackage(path.join(dir, 'a.ipk')), APP);
  fs.writeFileSync(path.join(dir, 'b.ipk'), 'not an archive');
  assert.strictEqual(update.ipkPackage(path.join(dir, 'b.ipk')), null);
});

test('a newer IPK downloads through a redirect, is checked and waits for Install', async () => {
  const dir = h.tmpDir();
  const key = keys(dir);
  const bytes = ipk(APP);
  const s = await site({ '/feed': sign(key, { webos: entry(bytes) }), '/ipk': bytes });
  const { u, states } = updater({ dir, keyPem: key.pub, get: s.get });
  const state = await u.check(true);
  s.close();
  assert.deepStrictEqual(state, { kind: 'ready', version: '0.5.0' });
  assert.deepStrictEqual(states[0], { kind: 'checking' });
  assert.ok(states.some((x) => x.kind === 'downloading' && x.percent === 100));
  const staged = fs.readdirSync(dir).filter((n) => n.indexOf('momotus-update-') === 0);
  assert.strictEqual(staged.length, 1);
  assert.strictEqual((fs.statSync(path.join(dir, staged[0])).mode & 0o777).toString(8), '700');
});

test('another app, a changed IPK, a bad signature or a lower version is never staged', async () => {
  const dir = h.tmpDir();
  const key = keys(dir);
  fs.mkdirSync(dir + '/o');
  const other = keys(dir + '/o');
  const mine = ipk(APP);
  // Same size, one byte changed: only the hash tells it apart.
  const changed = Buffer.from(mine);
  changed[changed.length - 3] ^= 1;
  const cases = [
    ['another package', { '/feed': sign(key, { webos: entry(ipk('app.other')) }), '/ipk': ipk('app.other') }, 'failed'],
    ['bytes not the feed hash', { '/feed': sign(key, { webos: entry(mine) }), '/ipk': changed }, 'failed'],
    ['larger than the feed says', { '/feed': sign(key, { webos: entry(mine) }), '/ipk': ipk(APP + 'x') }, 'failed'],
    ['another key', { '/feed': sign(other, { webos: entry(mine) }), '/ipk': mine }, 'failed'],
    ['not newer', { '/feed': sign(key, { webos: entry(mine, { version: '0.4.0' }) }), '/ipk': mine }, 'current'],
  ];
  for (const [name, files, want] of cases) {
    const s = await site(files);
    const { u } = updater({ dir, keyPem: key.pub, get: s.get });
    const state = await u.check(true);
    s.close();
    assert.strictEqual(state.kind, want, name);
  }
  assert.deepStrictEqual(fs.readdirSync(dir).filter((n) => n.indexOf('momotus-update-') === 0), []);
});

test('an unasked check that finds nothing stays quiet', async () => {
  const dir = h.tmpDir();
  const key = keys(dir);
  const s = await site({});
  const { u, states } = updater({ dir, keyPem: key.pub, get: s.get });
  assert.deepStrictEqual(await u.check(false), { kind: 'idle' });
  s.close();
  assert.deepStrictEqual(states, [{ kind: 'idle' }]);
});

// A luna-send-pub stand-in: install replies stream and the process never ends
// by itself (as -i); launch answers once. Each call is logged.
function fakeLuna(dir, installReplies) {
  const file = path.join(dir, 'luna-send-pub');
  fs.writeFileSync(
    file,
    '#!/usr/bin/env node\n' +
      'const fs = require("fs");\n' +
      'fs.appendFileSync(' + JSON.stringify(path.join(dir, 'calls')) + ', JSON.stringify(process.argv.slice(2)) + "\\n");\n' +
      'if (process.argv.indexOf("-i") >= 0) {\n' +
      '  for (const r of ' + JSON.stringify(installReplies) + ') console.log(JSON.stringify(r));\n' +
      '  setInterval(() => {}, 1000);\n' +
      '} else console.log(JSON.stringify({ returnValue: true }));\n',
  );
  fs.chmodSync(file, 0o755);
  return file;
}

function installRun(dir, replies) {
  const ipkDir = fs.mkdtempSync(path.join(dir, 'momotus-update-'));
  const file = path.join(ipkDir, 'momotus.ipk');
  fs.writeFileSync(file, ipk(APP));
  const luna = fakeLuna(dir, replies);
  const env = Object.assign({}, process.env, { MQ_LUNA_SEND: luna, MQ_INSTALL_LIMIT_MS: '3000' });
  const child = h.track(spawn(process.execPath, [path.join(__dirname, '..', 'update-install.js'), file, APP, path.join(dir, 'update.log')], { env }));
  return new Promise((resolve) => child.on('exit', (code) => resolve({ code, file })));
}

const calls = (dir) => fs.readFileSync(path.join(dir, 'calls'), 'utf8').trim().split('\n').map(JSON.parse);
const logged = (dir) => fs.readFileSync(path.join(dir, 'update.log'), 'utf8').trim().split('\n').map(JSON.parse);

test('the install child stops at statusValue 30, opens the app and removes the IPK', async () => {
  const dir = h.tmpDir();
  const r = await installRun(dir, [{ returnValue: true, subscribed: true }, { statusValue: 22, details: { progress: 50 } }, { statusValue: 30, details: { packageId: APP } }]);
  assert.strictEqual(r.code, 0);
  assert.ok(!fs.existsSync(r.file));
  const c = calls(dir);
  assert.strictEqual(c[0][1], 'luna://com.webos.appInstallService/dev/install');
  assert.strictEqual(JSON.parse(c[0][2]).ipkUrl, r.file);
  assert.deepStrictEqual(c[1], ['-n', '1', 'luna://com.webos.applicationManager/launch', JSON.stringify({ id: APP })]);
  assert.strictEqual(logged(dir).filter((l) => l.install)[0].install, 'installed');
});

test('a failed install opens nothing and says why in the log', async () => {
  const dir = h.tmpDir();
  const r = await installRun(dir, [{ returnValue: true, subscribed: true }, { statusValue: 25, details: { errorCode: -5, reason: 'no space' } }]);
  assert.strictEqual(r.code, 1);
  assert.strictEqual(calls(dir).length, 1);
  const line = logged(dir).filter((l) => l.install)[0];
  assert.deepStrictEqual([line.install, line.why], ['failed', 'no space']);
});

test('an install that never answers is given up after its limit', async () => {
  const dir = h.tmpDir();
  const r = await installRun(dir, [{ returnValue: true, subscribed: true }]);
  assert.strictEqual(r.code, 1);
  assert.strictEqual(logged(dir).filter((l) => l.install)[0].why, 'timed out');
});

test('Install starts the child once, detached, with the staged file', async () => {
  const dir = h.tmpDir();
  const key = keys(dir);
  const bytes = ipk(APP);
  const s = await site({ '/feed': sign(key, { webos: entry(bytes) }), '/ipk': bytes });
  const argsFile = path.join(dir, 'args');
  const recorder = path.join(dir, 'recorder.js');
  fs.writeFileSync(recorder, 'require("fs").writeFileSync(' + JSON.stringify(argsFile) + ', JSON.stringify(process.argv.slice(2)));\n');
  const { u } = updater({ dir, keyPem: key.pub, get: s.get, installer: [process.execPath, recorder] });
  await u.check(true);
  s.close();
  const reply = JSON.parse(await u.handle(JSON.stringify({ host: 'update', id: 7, op: 'install' })));
  assert.deepStrictEqual(reply, { host: 'update', id: 7, state: { kind: 'installing', version: '0.5.0' } });
  const args = await h.until('the child ran', () => fs.existsSync(argsFile) && JSON.parse(fs.readFileSync(argsFile, 'utf8')));
  assert.strictEqual(args[1], APP);
  assert.strictEqual(path.basename(args[0]), 'momotus.ipk');
  // A second Install while installing starts nothing new.
  fs.unlinkSync(argsFile);
  await u.handle(JSON.stringify({ host: 'update', id: 8, op: 'install' }));
  await h.sleep(300);
  assert.ok(!fs.existsSync(argsFile));
});

test('the service answers update requests on the page socket and never forwards them', async () => {
  const dir = h.tmpDir();
  const key = keys(dir);
  fs.writeFileSync(path.join(dir, 'pub.pem'), key.pub);
  const s = await site({ '/feed': sign(key, { webos: entry(ipk(APP), { version: '0.0.1' }) }) });
  const svc = await h.startService({ dir, env: { MQ_UPDATE_KEY: path.join(dir, 'pub.pem'), MQ_UPDATE_FEED: s.base + '/feed' } });
  const c = await h.authed(svc);
  c.send('{"host":"update","id":1,"op":"state"}');
  assert.deepStrictEqual(JSON.parse(await c.next()), { host: 'update', id: 1, state: { kind: 'idle' } });
  c.send('{"host":"update","id":2,"op":"check","asked":true}');
  // The pushed states, then the answer; the tree's package.json is 0.0.1, so nothing is newer.
  const seen = [];
  for (;;) {
    const m = JSON.parse(await c.next());
    seen.push(m);
    if (m.id === 2) break;
  }
  s.close();
  assert.deepStrictEqual(seen.map((m) => m.state.kind), ['checking', 'current', 'current']);
  assert.ok(seen.every((m) => m.host === 'update'));
  await svc.kill();
});

h.run();
