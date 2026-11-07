'use strict';
// A tiny runner and the helpers the service tests share. Plain Node 8 to 20,
// no framework: a test is `test(name, async fn)`, run one after another.
const assert = require('assert');
const fs = require('fs');
const http = require('http');
const net = require('net');
const os = require('os');
const path = require('path');
const { spawn, execFileSync } = require('child_process');
const WebSocket = require('ws');

const SERVICE = path.join(__dirname, '..', 'index.js');
const tracked = new Set();
const cases = [];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function until(what, f, ms) {
  const end = Date.now() + (ms || 8000);
  for (;;) {
    const v = await f();
    if (v) return v;
    if (Date.now() > end) throw new Error('timed out: ' + what);
    await sleep(25);
  }
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === 'EPERM';
  }
}

// Every process a test starts is killed by pid when the run ends.
function track(proc) {
  tracked.add(proc.pid);
  return proc;
}

process.on('exit', () => {
  tracked.forEach((pid) => {
    try {
      process.kill(pid, 'SIGKILL');
    } catch (e) {}
  });
  dirs.forEach((d) => execFileSync('rm', ['-rf', d]));
});

function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, '127.0.0.1', () => {
      const port = s.address().port;
      s.close(() => resolve(port));
    });
  });
}

const dirs = new Set();
function tmpDir() {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'mq-svc-test-'));
  dirs.add(d);
  return d;
}

// Starts the service with the fake Luna and the fake core. `env` adds to or
// overrides the defaults; a `dir` is reused across a restart.
async function startService(opts) {
  const o = opts || {};
  const dir = o.dir || tmpDir();
  const ui = path.join(dir, 'ui');
  if (!fs.existsSync(ui)) {
    fs.mkdirSync(ui);
    fs.writeFileSync(path.join(ui, 'index.html'), '<!doctype html><title>ui</title>');
    fs.writeFileSync(path.join(ui, 'app.js'), 'console.log(1)');
  }
  const data = path.join(dir, 'data');
  if (!fs.existsSync(data)) fs.mkdirSync(data);
  const port = o.port || (await freePort());
  const env = Object.assign({}, process.env, {
    MQ_LUNA: path.join(__dirname, 'fake-luna.js'),
    MQ_CORE: path.join(__dirname, 'fake-core.js'),
    MQ_DATA: data,
    MQ_UI: ui,
    MQ_PORT: String(port),
    MQ_GRACE_MS: '30000',
  }, o.env);
  const proc = track(spawn(process.execPath, (o.preload ? ['-r', path.join(__dirname, o.preload)] : []).concat(SERVICE), { env, stdio: ['ignore', 'pipe', 'inherit'] }));
  const svc = { proc, dir, data, port, events: [], exit: null, start: null };
  let buf = '';
  proc.stdout.on('data', (d) => {
    buf += d;
    const lines = buf.split('\n');
    buf = lines.pop();
    lines.forEach((l) => {
      try {
        svc.events.push(JSON.parse(l));
      } catch (e) {}
    });
  });
  proc.on('exit', (code) => (svc.exit = { code }));
  svc.find = (key) => svc.events.filter((e) => key in e);
  svc.waitFor = (key, n) => until('event ' + key, () => svc.find(key).length >= (n || 1));
  svc.kill = () => {
    try {
      process.kill(proc.pid, 'SIGKILL');
    } catch (e) {}
    return until('service exit', () => svc.exit);
  };
  await svc.waitFor('luna');
  svc.start = svc.find('luna')[0].response;
  svc.token = svc.start.token;
  return svc;
}

// A ws client: `messages` collects text, `closed` holds the close code.
function connect(svc, o) {
  const opts = o || {};
  const headers = {};
  if (opts.host) headers.Host = opts.host;
  const ws = new WebSocket('ws://127.0.0.1:' + svc.port + '/ws', {
    origin: opts.origin === undefined ? 'http://127.0.0.1:' + svc.port : opts.origin || undefined,
    headers,
  });
  const c = { ws, messages: [], closed: null };
  ws.on('message', (m) => c.messages.push(String(m)));
  ws.on('close', (code) => (c.closed = { code }));
  c.opened = new Promise((resolve, reject) => {
    ws.on('open', () => resolve(c));
    ws.on('error', reject);
  });
  c.send = (t) => ws.send(t);
  c.next = (what) => until(what || 'a message', () => c.messages.shift(), 8000);
  return c;
}

async function authed(svc) {
  const c = connect(svc);
  await c.opened;
  c.send(svc.token);
  return c;
}

// Connects and sends `pid` until the core answers (it is up and ready).
async function corePid(svc) {
  const c = await authed(svc);
  c.send('pid');
  const m = JSON.parse(await c.next('pid answer'));
  return { c, pid: m.pid };
}

function get(svc, urlPath, headers) {
  return new Promise((resolve, reject) => {
    http
      .get({ host: '127.0.0.1', port: svc.port, path: urlPath, headers: headers || {} }, (res) => {
        const parts = [];
        res.on('data', (d) => parts.push(d));
        res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(parts).toString() }));
      })
      .on('error', reject);
  });
}

// assert.rejects does not exist on Node 8.
async function rejects(promise) {
  try {
    await promise;
  } catch (e) {
    return e;
  }
  throw new assert.AssertionError({ message: 'expected a rejection' });
}

function test(name, fn) {
  cases.push({ name, fn });
}

async function run() {
  let failed = 0;
  for (const t of cases) {
    try {
      await t.fn();
      console.log('ok   ' + t.name);
    } catch (e) {
      failed += 1;
      console.log('FAIL ' + t.name + '\n     ' + String((e && e.stack) || e).split('\n').join('\n     '));
    }
  }
  console.log((failed ? 'FAILED ' + failed + ' of ' : 'passed ') + cases.length + ' (node ' + process.version + ')');
  process.exit(failed ? 1 : 0);
}

module.exports = { rejects, assert, test, run, sleep, until, alive, track, startService, connect, authed, corePid, get, tmpDir, freePort };
