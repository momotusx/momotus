'use strict';
// The core child (plans/webos.md 1): runs the wasm core for the service's
// parent, over the fork IPC channel in the ToChild and ToParent unions of
// host.d.ts. `MQ_CORE` replaces the core module for tests.
const path = require('path');
const host = require('./host.js');

let core = null;
host.install(() => core);
// `died` runs once; the parent then replaces this process.
host.setOnDied((message) => {
  if (!process.connected) return process.exit(3);
  process.send({ kind: 'died', message: String(message).slice(0, 500) }, () => process.exit(3));
});

const send = (m) => process.send(m);

function open(config) {
  core = require(process.env.MQ_CORE || path.join(__dirname, 'core', 'mq_wasm.js'));
  core.onMessage((text) => send({ kind: 'socket', text }));
  send({ kind: 'ready', open: core.open(JSON.stringify(config)) });
}

function serve(id, requestPath) {
  const toParent = (answer) => send({ kind: 'served', id, answer });
  core.serve(requestPath).then(
    (a) =>
      toParent(a && { status: a.status, headers: a.headers, body: Buffer.from(a.body.buffer, a.body.byteOffset, a.body.byteLength).toString('base64') }),
    () => toParent(null),
  );
}

process.on('message', (m) => {
  if (m.kind === 'open') open(m.config);
  else if (m.kind === 'socket') core.send(m.text);
  else if (m.kind === 'serve') serve(m.id, m.path);
});
