#!/usr/bin/env node
'use strict';
// A stand-in for the native core (MQ_NATIVE): mq-bridge's ready line, its
// token check, a session per socket, and exit on stdin EOF. FAKE_BRIDGE picks
// how it misbehaves: `silent` never says ready, `exit` exits at once,
// `orphan` outlives its parent (stays up when its stdin closes).
// Socket texts: `env` answers what it was started with, `n` this socket's
// session number, `open` the sessions open now, `big:<n>` an answer of n
// bytes, `die` exits the process, anything else is echoed.
const WebSocket = require('ws');

const mode = process.env.FAKE_BRIDGE || '';
if (mode === 'exit') process.exit(2);
if (process.env.MQ_BRIDGE_EXIT_ON_EOF && mode !== 'orphan') {
  process.stdin.on('data', () => {});
  process.stdin.on('end', () => process.exit(0));
}
const port = Number(process.env.MQ_BRIDGE_PORT);
let sessions = 0;
let open = 0;
const wss = new WebSocket.Server({
  host: '127.0.0.1',
  port,
  verifyClient: (info) => info.req.headers['x-mq-token'] === process.env.MQ_BRIDGE_TOKEN,
});
wss.on('listening', () => {
  if (mode !== 'silent') console.log(JSON.stringify({ event: 'bridge_ready', url: 'ws://127.0.0.1:' + port + '/ws', origins: [] }));
});
wss.on('connection', (ws) => {
  const session = (sessions += 1);
  open += 1;
  ws.on('close', () => (open -= 1));
  ws.on('message', (text) => {
    if (text === 'die') return process.exit(3);
    if (text === 'pid') return ws.send(JSON.stringify({ pid: process.pid }));
    if (text === 'n') return ws.send('session:' + session);
    if (text === 'open') return ws.send('open:' + open);
    if (text.indexOf('big:') === 0) return ws.send('x'.repeat(Number(text.slice(4))));
    if (text.length > 1000) return ws.send('got:' + text.length);
    if (text === 'env') {
      const e = process.env;
      return ws.send(JSON.stringify({ argv: process.argv.slice(2), data: e.MQ_DATA, port: e.MQ_BRIDGE_PORT, token: e.MQ_BRIDGE_TOKEN, certs: e.SSL_CERT_FILE || null }));
    }
    ws.send('native:' + text);
  });
});
