'use strict';
// The native core (plans/webos.md v5): the mq-bridge binary, started by the
// service, with the torrent engine and Nuvio plugins. `start` returns an
// object shaped like the forked wasm child (host.d.ts ToChild and ToParent:
// `send`, `kill`, `pid`, and `message`, `error`, `exit` events), so the
// service's queue, restart and pid rules hold for both. Each page socket gets
// a socket of its own to the bridge, so each has its own Session there, as
// with the wasm core. Video and subtitles never pass through here: the
// bridge's leases point at its own loopback stream server.
const EventEmitter = require('events');
const { spawn } = require('child_process');
const WebSocket = require('ws');

const TOKEN_HEADER = 'x-mq-token';

// `bin` with `args`, its env holding MQ_BRIDGE_TOKEN and, with
// MQ_BRIDGE_PORT 0, binding a free port it names in its `bridge_ready` line.
// It is ready once it prints that line. Its `bridge_opening` heartbeat
// (MQ_BRIDGE_HEARTBEAT_MS) keeps a slow first open alive: it is killed after
// `quietMs` with no line at all, or `capMs` in all. `c.why` says which, and
// `c.stderr` holds its last lines (a panic's message).
function start(bin, args, env, quietMs, capMs) {
  const c = new EventEmitter();
  let url = null;
  const upstreams = new Map(); // page socket id -> { ws, queue }
  let ready = false;
  c.stderr = [];
  let proc;
  try {
    proc = spawn(bin, args, { env, stdio: ['pipe', 'pipe', 'pipe'] });
  } catch (e) {
    // Some Nodes throw at once for a file they cannot run: the same error event.
    c.kill = () => {};
    c.send = (m, done) => done && done();
    setImmediate(() => c.emit('error', e));
    return c;
  }
  c.pid = proc.pid;
  c.kill = (signal) => proc.kill(signal);
  const giveUp = (why) => {
    if (ready) return;
    c.why = why;
    proc.kill('SIGKILL');
  };
  let quiet = null;
  const heard = () => {
    clearTimeout(quiet);
    quiet = setTimeout(() => giveUp('no line in ' + quietMs + ' ms'), quietMs);
  };
  heard();
  const cap = setTimeout(() => giveUp('not ready in ' + capMs + ' ms'), capMs);
  const timer = { clear: () => (clearTimeout(quiet), clearTimeout(cap)) };
  // The bridge exits when this pipe closes (MQ_BRIDGE_EXIT_ON_EOF); a write
  // to a gone process must not throw here.
  proc.stdin.on('error', () => {});
  proc.on('error', (e) => {
    timer.clear();
    c.emit('error', e);
  });
  proc.on('exit', (code, signal) => {
    timer.clear();
    upstreams.forEach((u) => u.ws.terminate());
    upstreams.clear();
    c.emit('exit', code === null ? signal : code);
  });
  lines(proc.stdout, (line) => {
    let m = null;
    try {
      m = JSON.parse(line);
    } catch (e) {}
    if (ready || !m) return;
    if (m.event === 'bridge_opening') return heard();
    const port = m.event === 'bridge_ready' && /^ws:\/\/127\.0\.0\.1:(\d+)\/ws$/.exec(m.url || '');
    if (!port) return;
    ready = true;
    timer.clear();
    url = m.url;
    c.emit('message', { kind: 'ready', open: { native: true, port: Number(port[1]) } });
  });
  lines(proc.stderr, (line) => {
    c.stderr = c.stderr.concat(line.slice(0, 300)).slice(-3);
    console.log(JSON.stringify({ nativeLog: line.slice(0, 500) }));
  });

  function upstream(socket) {
    const old = upstreams.get(socket);
    if (old) return old;
    const ws = new WebSocket(url, { headers: { [TOKEN_HEADER]: env.MQ_BRIDGE_TOKEN }, perMessageDeflate: false });
    const u = { ws, queue: [] };
    upstreams.set(socket, u);
    ws.on('open', () => u.queue.splice(0).forEach((q) => ws.send(q.text, q.done)));
    ws.on('message', (text) => c.emit('message', { kind: 'socket', socket, text: String(text) }));
    // The bridge ended this Session (1013 past its output budget, or it
    // went away): the page socket closes too, and the page reloads.
    ws.on('close', (code) => {
      u.queue.splice(0).forEach((q) => q.done());
      if (upstreams.get(socket) !== u) return;
      upstreams.delete(socket);
      c.emit('message', { kind: 'socket', socket, text: null, code: code === 1013 ? undefined : 1012 });
    });
    ws.on('error', () => {});
    return u;
  }

  // The service's messages: `open` carries the wasm core's config (the
  // bridge has its env), `serve` has nothing to serve here (404).
  c.send = (m, done) => {
    const ack = once(done);
    if (m.kind === 'socket') {
      const u = upstream(m.socket);
      if (u.ws.readyState === WebSocket.OPEN) u.ws.send(m.text, ack);
      else u.queue.push({ text: m.text, done: ack });
      return true;
    }
    if (m.kind === 'close') {
      const u = upstreams.get(m.socket);
      upstreams.delete(m.socket);
      if (u) u.ws.close();
    } else if (m.kind === 'serve') {
      setImmediate(() => c.emit('message', { kind: 'served', id: m.id, answer: null }));
    }
    ack();
    return true;
  };
  return c;
}

function once(f) {
  let called = false;
  return () => {
    if (called || !f) return;
    called = true;
    f();
  };
}

function lines(stream, onLine) {
  let buf = '';
  stream.setEncoding('utf8');
  stream.on('data', (d) => {
    buf += d;
    const parts = buf.split('\n');
    buf = parts.pop();
    parts.forEach((l) => l && onLine(l));
  });
}

module.exports = { start };
