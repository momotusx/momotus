'use strict';
// The Momotus webOS service (plans/webos.md section 1). Luna `start` answers
// the stub with the token; the http server on 127.0.0.1 serves the built UI and
// forwards /sub/ and /logo/ to the core child; the ws at /ws carries the
// Session after a token; the keystore lives here, never in the core.
// The core is the native binary bin/mq-bridge where it starts (torrents and
// plugins; plans/webos.md v5), else the wasm core in a forked child.
// The MQ_* variables are for tests and simulators: MQ_DATA, MQ_CORE, MQ_UI,
// MQ_PORT, MQ_GRACE_MS, MQ_LUNA (a stand-in for webos-service), MQ_SIM,
// MQ_NATIVE (another native binary, or empty for none), MQ_NATIVE_QUIET_MS and
// MQ_NATIVE_CAP_MS (its start's silence and total limits).
const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const path = require('path');
const { execFile, fork } = require('child_process');
const WebSocket = require('ws');
const keystore = require('./keystore.js');
const native = require('./native.js');

const APP = 'app.momotus.webos';
const PORT = Number(process.env.MQ_PORT) || 47801;
const ORIGIN = 'http://127.0.0.1:' + PORT;
const GRACE_MS = Number(process.env.MQ_GRACE_MS) || 30000;
const MAX_RESTARTS = 3;
const RESTART_WINDOW_MS = 60000;
const MAX_QUEUE = 1000;
const MAX_BUFFERED = 8 * 1024 * 1024;
const CLOSE_BAD_TOKEN = 4401;
const NATIVE_BIN = process.env.MQ_NATIVE !== undefined ? process.env.MQ_NATIVE : path.join(__dirname, 'bin', 'mq-bridge');
// The native core's start: dead after 20 s with no line, or 90 s in all (a slow
// TV's first open sends its heartbeat every 2 s meanwhile).
const NATIVE_QUIET_MS = Number(process.env.MQ_NATIVE_QUIET_MS) || 20000;
const NATIVE_CAP_MS = Number(process.env.MQ_NATIVE_CAP_MS) || 90000;

const Service = require(process.env.MQ_LUNA || 'webos-service');
const service = new Service(APP + '.service');
const token = crypto.randomBytes(32).toString('hex');
// The bridge's socket token: no other app's service on the TV gets a Session.
const bridgeToken = crypto.randomBytes(32).toString('hex');
const dataDir = process.env.MQ_DATA || '/media/internal/' + APP;
const uiDir = process.env.MQ_UI || path.join(__dirname, 'ui');
const pidFile = path.join(dataDir, 'core.pid');

const sockets = new Set(); // authenticated
// Each authenticated socket has its own Session in the core, as a socket to
// the native bridge does: its id goes with its text, and its close ends it.
const byId = new Map(); // socket id -> ws
let socketId = 0;
const pending = new Map(); // serve id -> http response
let store = null;
let listening = null;
let child = null;
let ready = false;
let queue = [];
let queueBytes = 0;
let restarts = [];
let serveId = 0;
let keep = null;
let keepCreating = false;
let wantAlive = false;
let graceTimer = null;
// The write probe's result: a failed one runs the core's library in memory.
let probed = { ok: true };
// 'native' until the binary fails to start or keeps dying, then 'wasm' for the
// rest of this process. The page learns it from index.html and says it back
// on its socket, so a page loaded before a switch reloads into the new one.
let coreKind = NATIVE_BIN && fs.existsSync(NATIVE_BIN) ? 'native' : 'wasm';

const log = (entry) => console.log(JSON.stringify(entry));

// Keep-alive (platform r2 1): one ActivityManager activity, held while an
// authenticated socket is open, completed GRACE_MS after the last one closes
// or after `start` if none ever authenticates.
function syncKeep() {
  if (wantAlive && !keep && !keepCreating) {
    keepCreating = true;
    service.activityManager.create('keepAlive', (activity) => {
      keep = activity;
      keepCreating = false;
      syncKeep();
    });
  } else if (!wantAlive && keep && !keepCreating) {
    const done = keep;
    keep = null;
    service.activityManager.complete(done, () => {});
  }
}

function holdAlive() {
  clearTimeout(graceTimer);
  wantAlive = true;
  syncKeep();
}

function releaseLater() {
  clearTimeout(graceTimer);
  graceTimer = setTimeout(() => {
    if (sockets.size === 0) {
      wantAlive = false;
      syncKeep();
    }
  }, GRACE_MS);
}

function probe() {
  const t = Date.now();
  try {
    if (!fs.existsSync(dataDir)) fs.mkdirSync(dataDir);
    const tmp = path.join(dataDir, 'probe.tmp');
    const fd = fs.openSync(tmp, 'w');
    fs.writeSync(fd, 'probe ' + t);
    fs.fsyncSync(fd);
    fs.closeSync(fd);
    fs.renameSync(tmp, path.join(dataDir, 'probe'));
    return { ok: fs.readFileSync(path.join(dataDir, 'probe'), 'utf8') === 'probe ' + t, ms: Date.now() - t };
  } catch (e) {
    return { ok: false, error: String(e.code || e) };
  }
}

// A persistent seed the core derives the device id from: 64 hex chars, or a
// new one, written atomically.
function deviceSeed() {
  const file = path.join(dataDir, 'device-seed');
  try {
    const seed = fs.readFileSync(file, 'utf8');
    if (/^[0-9a-f]{64}$/.test(seed)) return seed;
  } catch (e) {}
  const seed = crypto.randomBytes(32).toString('hex');
  try {
    keystore.writeAtomic(file, seed);
  } catch (e) {
    log({ seedWrite: String(e.code || e) });
  }
  return seed;
}

// ---- core child ----

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === 'EPERM';
  }
}

// Only this install's own child counts: its script and this data dir, as
// whole arguments.
function isOurWorker(command) {
  const args = command.split(/\s+/);
  const ours = args.indexOf(path.join(__dirname, 'core-worker.js')) !== -1 || (!!NATIVE_BIN && args.indexOf(NATIVE_BIN) !== -1);
  return ours && args.indexOf(dataDir) !== -1;
}

function commandOf(pid, cb) {
  fs.readFile('/proc/' + pid + '/cmdline', 'utf8', (err, text) => {
    if (!err) return cb(text.replace(/\0/g, ' '));
    execFile('ps', ['-o', 'command=', '-p', String(pid)], (e, out) => cb(e ? '' : out));
  });
}

// A pid left in core.pid counts only while its command line is this install's
// core-worker (the file survives a reboot and the pid may be reused). It gets
// SIGTERM, then SIGKILL after 2 s; no new core starts while it lives, since two
// cores would write the unlocked database. One still alive after 5 s stops the
// service (the page's relaunch tries again, then shows the could-not-start
// line). A process we may not signal (EPERM) is not ours: the start goes on.
function sweepStale(done) {
  let pid = NaN;
  try {
    pid = parseInt(fs.readFileSync(pidFile, 'utf8'), 10);
  } catch (e) {}
  if (!(pid > 0) || pid === process.pid) return done();
  commandOf(pid, (command) => {
    if (!isOurWorker(command) || !isAlive(pid)) return done();
    log({ stale: pid });
    try {
      process.kill(pid, 'SIGTERM');
    } catch (e) {
      return done();
    }
    const t0 = Date.now();
    const poll = setInterval(() => {
      if (!isAlive(pid)) {
        clearInterval(poll);
        return done();
      }
      if (Date.now() - t0 >= 5000) {
        clearInterval(poll);
        return stop('stale core alive');
      }
      if (Date.now() - t0 >= 2000) {
        try {
          process.kill(pid, 'SIGKILL');
        } catch (e) {}
      }
    }, 50);
  });
}

function closeSockets(code) {
  sockets.forEach((ws) => ws.close(code, 'core restart'));
  pending.forEach((res) => {
    res.writeHead(503);
    res.end();
  });
  pending.clear();
}

function stop(why) {
  log({ stop: why });
  closeSockets(1011);
  setTimeout(() => process.exit(1), 100);
}

// The bridge gets the data dir as its argument only so a stale core.pid
// can be told apart (isOurWorker); its own files sit in native/, never in
// the wasm core's database (that one has no WAL).
// An update from the wasm-only IPK (v0.3.0 and before): the native core's
// first start takes the wasm core's library, once, so history, addons and
// settings stay. Both cores are mq-library from the same IPK (one schema, its
// migrations run forward on open). The wasm core is not running here (one
// core at a time, core.pid), so the file is as SQLite left it: TRUNCATE mode,
// no -wal or -shm. A journal of zero bytes is a finished commit and stays
// behind; a non-empty one is a cut transaction, which goes along so the first
// open rolls it back as it would have in place. The native core then turns
// the file to WAL. The database is copied last: its presence marks the copy
// done, and a copy cut short leaves only a .tmp, tried again next start.
function adoptWasmLibrary(nativeDir) {
  const from = path.join(dataDir, 'library.sqlite');
  const to = path.join(nativeDir, 'library.sqlite');
  if (fs.existsSync(to) || !fs.existsSync(from)) return;
  try {
    if (!fs.existsSync(nativeDir)) fs.mkdirSync(nativeDir);
    let cut = false;
    try {
      cut = fs.statSync(from + '-journal').size > 0;
    } catch (e) {}
    if (cut) copyDurable(from + '-journal', to + '-journal');
    copyDurable(from, to);
    log({ adopted: { bytes: fs.statSync(to).size, journal: cut } });
  } catch (e) {
    log({ adoptFailed: String(e.code || e) });
  }
}

function copyDurable(src, dst) {
  const tmp = dst + '.tmp';
  fs.writeFileSync(tmp, fs.readFileSync(src));
  const fd = fs.openSync(tmp, 'r+');
  fs.fsyncSync(fd);
  fs.closeSync(fd);
  fs.renameSync(tmp, dst);
}

function startNative() {
  try {
    fs.accessSync(NATIVE_BIN, fs.constants.X_OK);
  } catch (e) {
    try {
      fs.chmodSync(NATIVE_BIN, 0o755);
    } catch (e2) {
      log({ nativeChmod: String(e2.code || e2) });
    }
  }
  const nativeDir = path.join(dataDir, 'native');
  adoptWasmLibrary(nativeDir);
  const env = Object.assign({}, process.env, {
    MQ_DATA: nativeDir,
    // A free port, named in its ready line: a taken one never stops it.
    MQ_BRIDGE_PORT: '0',
    MQ_BRIDGE_TOKEN: bridgeToken,
    MQ_BRIDGE_EXIT_ON_EOF: '1',
    MQ_BRIDGE_HEARTBEAT_MS: '2000',
    // The device id the wasm core uses (mq-wasm device_id), so both write as one device.
    MQ_DEVICE_ID: deviceSeed().slice(0, 16),
  });
  // The TV's own CA store may be out of the app's reach: Node's roots,
  // shipped by package.sh, are the bridge's.
  const roots = path.join(__dirname, 'cacert.pem');
  if (!env.SSL_CERT_FILE && fs.existsSync(roots)) env.SSL_CERT_FILE = roots;
  return native.start(NATIVE_BIN, [dataDir], env, NATIVE_QUIET_MS, NATIVE_CAP_MS);
}

function spawnChild() {
  const c = coreKind === 'native' ? startNative() : fork(path.join(__dirname, 'core-worker.js'), [dataDir], { execArgv: [] });
  c.native = coreKind === 'native';
  child = c;
  ready = false;
  c.retiring = false;
  c.exited = false;
  c.inflight = 0; // bytes handed to c.send and not yet written to the pipe
  try {
    if (c.pid) fs.writeFileSync(pidFile, String(c.pid));
  } catch (e) {
    log({ pidWrite: String(e.code || e) });
  }
  const config = { dataDir, deviceSeed: deviceSeed(), node: process.version, origin: ORIGIN, memoryOnly: !probed.ok };
  if (process.env.MQ_DEV) config.dev = JSON.parse(process.env.MQ_DEV);
  c.send({ kind: 'open', config });
  // Handlers are per child: a late event from an old one never touches the
  // new one.
  c.on('message', (m) => {
    if (child !== c || c.retiring) return;
    if (m.kind === 'ready') {
      ready = true;
      c.wasReady = true;
      log({ ready: true, core: coreKind, open: m.open });
      queue.forEach((q) => sendToChild(c, q.socket, q.text));
      queue = [];
      queueBytes = 0;
    } else if (m.kind === 'socket') {
      const ws = byId.get(m.socket);
      if (!ws || ws.readyState !== WebSocket.OPEN) return;
      // `text: null`: the session stopped past its output budget, or (with a
      // `code`) the native core ended it.
      if (m.text === null) ws.close(m.code || 1013, m.code ? 'core closed' : 'overflow');
      else if (ws.bufferedAmount > MAX_BUFFERED) ws.close(1013, 'slow reader');
      else ws.send(m.text);
    } else if (m.kind === 'served') {
      answerServe(m);
    } else if (m.kind === 'died') {
      retire(c, 'died: ' + m.message);
    }
  });
  c.on('error', (e) => {
    if (!c.pid) c.exited = true;
    c.ended = 'error ' + String(e.code || e);
    retire(c, 'error: ' + e);
    if (c.exited) gone(c);
  });
  c.on('exit', (code) => {
    c.exited = true;
    c.ended = 'exit ' + code;
    clearTimeout(c.killTimer);
    if (code === 0) removePid(c.pid);
    retire(c, 'exit ' + code);
    gone(c);
  });
}

function removePid(pid) {
  try {
    if (parseInt(fs.readFileSync(pidFile, 'utf8'), 10) === pid) fs.unlinkSync(pidFile);
  } catch (e) {}
}

// The old child ends first (the database has no locking, so two cores must
// never hold the file): SIGTERM, SIGKILL after 2 s, a new fork after `exit`.
function retire(c, why) {
  if (c.retiring) return;
  c.retiring = true;
  log({ retire: why });
  if (child === c) {
    ready = false;
    queue = [];
    queueBytes = 0;
  }
  closeSockets(1012);
  if (c.exited) return;
  c.kill('SIGTERM');
  c.killTimer = setTimeout(() => c.kill('SIGKILL'), 2000);
}

function gone(c) {
  if (child !== c) return;
  child = null;
  ready = false;
  const now = Date.now();
  restarts = restarts.filter((t) => now - t < RESTART_WINDOW_MS);
  // A native core that never got ready (exec refused, another arch, a crash
  // at start) or keeps dying gives way to the wasm core: the page reloads
  // into its caps (direct sources only).
  if (c.native && (!c.wasReady || restarts.length >= MAX_RESTARTS)) {
    log({ native: 'off', why: c.wasReady ? 'restart cap' : 'never ready', detail: c.why || c.ended || null, stderr: c.stderr || [] });
    coreKind = 'wasm';
    restarts = [];
    return spawnChild();
  }
  if (restarts.length >= MAX_RESTARTS) return stop('restart cap');
  restarts.push(now);
  spawnChild();
}

function startCore() {
  sweepStale(spawnChild);
}

// `send` returning false is backpressure, not refusal: Node queues the message.
// A wedged child would let that queue grow without end, so the bytes handed
// over and not yet written are counted and bounded.
function sendToChild(c, socket, text) {
  const n = Buffer.byteLength(text);
  c.inflight += n;
  c.send({ kind: 'socket', socket, text }, () => {
    c.inflight -= n;
  });
}

// False when the text is refused: the caller closes the socket with 1013.
function toCore(socket, text) {
  const n = Buffer.byteLength(text);
  if (ready) {
    if (child.inflight + n > MAX_BUFFERED) return false;
    sendToChild(child, socket, text);
    return true;
  }
  if (queue.length >= MAX_QUEUE || queueBytes + n > MAX_BUFFERED) return false;
  queue.push({ socket, text });
  queueBytes += n;
  return true;
}

function answerServe(m) {
  const res = pending.get(m.id);
  if (!res) return;
  pending.delete(m.id);
  if (!m.answer) {
    res.writeHead(404);
    return res.end();
  }
  try {
    const status = m.answer.status;
    if (!(status >= 100 && status <= 999) || status % 1) throw new Error('bad status');
    m.answer.headers.forEach((h) => {
      const old = res.getHeader(h[0]);
      res.setHeader(h[0], old === undefined ? h[1] : [].concat(old, h[1]));
    });
    // Subtitles and logos are data: never sniffed, never run as a page.
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Content-Security-Policy', 'sandbox');
    const body = Buffer.from(m.answer.body, 'base64');
    res.statusCode = status;
    res.end(body);
  } catch (e) {
    log({ badServe: String(e.message || e) });
    res.getHeaderNames().forEach((n) => res.removeHeader(n));
    res.statusCode = 502;
    res.end();
  }
}

// ---- http and ws ----

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.webmanifest': 'application/manifest+json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.wasm': 'application/wasm',
  '.txt': 'text/plain; charset=utf-8',
};

// The page's mark that the native core runs (apps/web lib/webos.ts nativeCore):
// in <head>, or right after the doctype when the file has no <head> tag.
function markNative(html) {
  const meta = '<meta name="mq-core" content="native">';
  const head = /<head(\s[^>]*)?>/i;
  return head.test(html) ? html.replace(head, (m) => m + meta) : html.replace(/^(<!doctype[^>]*>)?/i, (m) => m + meta);
}

// The built UI with a fallback to index.html for client routes (paths with no
// extension); a missing file with one is a 404, never HTML served as a script.
function serveUi(req, res) {
  let rel;
  try {
    rel = decodeURIComponent(req.url.split('?')[0]);
  } catch (e) {
    res.writeHead(400);
    return res.end();
  }
  if (rel.indexOf('\0') !== -1) {
    res.writeHead(400);
    return res.end();
  }
  const file = path.normalize(path.join(uiDir, rel));
  if (file !== uiDir && file.indexOf(uiDir + path.sep) !== 0) {
    res.writeHead(403);
    return res.end();
  }
  const send = (target, fallback) => {
    // readFile throws at once on an argument it refuses.
    try {
      readUi(target, fallback);
    } catch (e) {
      res.writeHead(400);
      res.end();
    }
  };
  const readUi = (target, fallback) =>
    fs.readFile(target, (err, data) => {
      if (err) {
        if (fallback) return send(path.join(uiDir, 'index.html'), false);
        res.writeHead(404);
        return res.end();
      }
      const type = TYPES[path.extname(target).toLowerCase()] || 'application/octet-stream';
      const headers = { 'Content-Type': type, 'X-Content-Type-Options': 'nosniff' };
      if (type.indexOf('text/html') === 0) {
        headers['Content-Security-Policy'] = "frame-ancestors 'none'";
        headers['Cache-Control'] = 'no-cache';
        if (coreKind === 'native') data = Buffer.from(markNative(String(data)));
      }
      res.writeHead(200, headers);
      res.end(data);
    });
  send(file, path.extname(file) === '');
}

function onRequest(req, res) {
  if (req.headers.host !== '127.0.0.1:' + PORT) {
    res.writeHead(421);
    return res.end();
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405);
    return res.end();
  }
  const url = req.url.split('?')[0];
  if (url.indexOf('/sub/') === 0 || url.indexOf('/logo/') === 0) {
    if (!ready) {
      res.writeHead(503);
      return res.end();
    }
    const id = (serveId += 1);
    pending.set(id, res);
    res.on('close', () => pending.delete(id));
    return child.send({ kind: 'serve', id, path: req.url });
  }
  serveUi(req, res);
}

function sameToken(text) {
  const a = Buffer.from(text);
  const b = Buffer.from(token);
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

// A keystore request is answered here and never forwarded.
function onSocketText(ws, text) {
  if (text.indexOf(keystore.PREFIX) === 0) {
    return store.handle(text).then((reply) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(reply);
    });
  }
  if (!toCore(ws.coreId, text)) ws.close(1013, 'busy');
}

// Before the token a socket may send only the token (64 chars; a longer text
// is a bad token, 4401). Its receiver refuses anything over 256 bytes with
// 1009 before buffering it. ws 7.5.10 keeps one maxPayload per server, so the
// per-socket limit sits on its receiver.
const TOKEN_LEN = 64;
const PRE_AUTH_MAX = 256;

function onSocket(ws, req) {
  let authed = false;
  const pageCore = /[?&]core=native(&|$)/.test(req.url) ? 'native' : 'wasm';
  ws._receiver._maxPayload = PRE_AUTH_MAX;
  const lateTimer = setTimeout(() => authed || ws.close(CLOSE_BAD_TOKEN, 'token'), 10000);
  ws.on('message', (data) => {
    if (typeof data !== 'string') return ws.close(1003, 'text only');
    if (!authed) {
      if (data.length > TOKEN_LEN || !sameToken(data)) return ws.close(CLOSE_BAD_TOKEN, 'token');
      // A page built for the other core reloads (any close after the token does).
      if (pageCore !== coreKind) return ws.close(1012, 'core changed');
      authed = true;
      ws._receiver._maxPayload = MAX_BUFFERED;
      ws.coreId = socketId += 1;
      byId.set(ws.coreId, ws);
      sockets.add(ws);
      holdAlive();
      return;
    }
    onSocketText(ws, data);
  });
  ws.on('close', () => {
    clearTimeout(lateTimer);
    if (!sockets.delete(ws)) return;
    byId.delete(ws.coreId);
    // Its session ends with it; text it left in the queue goes too.
    if (ready) child.send({ kind: 'close', socket: ws.coreId });
    else {
      queue = queue.filter((q) => q.socket !== ws.coreId);
      queueBytes = queue.reduce((n, q) => n + Buffer.byteLength(q.text), 0);
    }
    if (sockets.size === 0) releaseLater();
  });
  ws.on('error', () => ws.terminate());
}

function serve() {
  return new Promise((resolve) => {
    const server = http.createServer(onRequest);
    const wss = new WebSocket.Server({ noServer: true, maxPayload: MAX_BUFFERED });
    server.on('upgrade', (req, socket, head) => {
      if (
        req.headers.host !== '127.0.0.1:' + PORT ||
        req.headers.origin !== ORIGIN ||
        req.url.split('?')[0] !== '/ws'
      ) {
        return socket.destroy();
      }
      wss.handleUpgrade(req, socket, head, onSocket);
    });
    server.once('error', (e) => resolve({ ok: false, error: String(e.code || e) }));
    server.listen(PORT, '127.0.0.1', () => {
      // After listen an error must not crash the service.
      server.on('error', (e) => log({ serverError: String(e.code || e) }));
      resolve({ ok: true });
    });
  });
}

// ---- Luna ----

function begin() {
  const wrote = probe();
  probed = wrote;
  store = keystore.create(dataDir);
  return serve().then((r) => {
    log({ listen: r, probe: wrote, node: process.version });
    if (r.ok) startCore();
    return Object.assign({ probe: wrote }, r);
  });
}

service.register('start', (message) => {
  // The Simulators report the service's own id as the sender; MQ_SIM accepts
  // that there only.
  const sim = process.env.MQ_SIM === '1' && message.sender === APP + '.service';
  if (message.sender !== APP && !sim) {
    return message.respond({ returnValue: false, errorText: 'not our app' });
  }
  if (!listening) listening = begin();
  holdAlive();
  listening.then((r) => {
    // A failed listen (EADDRINUSE) is tried again on the next `start`.
    if (!r.ok) listening = null;
    // No socket within the grace releases the keep-alive anyway, also after
    // a failed listen.
    if (sockets.size === 0) releaseLater();
    if (!r.ok) return message.respond({ returnValue: false, errorText: r.error });
    message.respond({ returnValue: true, token, probe: r.probe });
  });
});
