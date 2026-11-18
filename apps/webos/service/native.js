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

// `bin` with `args`, its env holding MQ_BRIDGE_PORT and MQ_BRIDGE_TOKEN; it
// is ready once it prints its `bridge_ready` line, and is killed if that
// takes over `readyMs`.
function start(bin, args, env, readyMs) {
  const c = new EventEmitter();
  const url = 'ws://127.0.0.1:' + env.MQ_BRIDGE_PORT + '/ws';
  const upstreams = new Map(); // page socket id -> { ws, queue }
  let ready = false;
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
  const timer = setTimeout(() => ready || proc.kill('SIGKILL'), readyMs);
  // The bridge exits when this pipe closes (MQ_BRIDGE_EXIT_ON_EOF); a write
  // to a gone process must not throw here.
  proc.stdin.on('error', () => {});
  proc.on('error', (e) => {
    clearTimeout(timer);
    c.emit('error', e);
  });
  proc.on('exit', (code, signal) => {
    clearTimeout(timer);
    upstreams.forEach((u) => u.ws.terminate());
    upstreams.clear();
    c.emit('exit', code === null ? signal : code);
  });
  lines(proc.stdout, (line) => {
    let m = null;
    try {
      m = JSON.parse(line);
    } catch (e) {}
    if (!ready && m && m.event === 'bridge_ready') {
      ready = true;
      clearTimeout(timer);
      c.emit('message', { kind: 'ready', open: { native: true } });
    }
  });
  lines(proc.stderr, (line) => console.log(JSON.stringify({ nativeLog: line.slice(0, 500) })));

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
