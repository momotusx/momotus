'use strict';
// A stand-in core module (MQ_CORE): echoes socket text and dies on request.
// FAKE_SLOW_MS delays `open` on every start after the first.
const fs = require('fs');
const path = require('path');

let cb = null;
let lastConfig = null;

exports.open = (configJson) => {
  const config = JSON.parse(configJson);
  lastConfig = config;
  const marker = path.join(config.dataDir, 'fake-started');
  if (fs.existsSync(marker)) {
    const until = Date.now() + Number(process.env.FAKE_SLOW_MS || 0);
    while (Date.now() < until) {}
  } else {
    fs.writeFileSync(marker, '1');
  }
  return JSON.stringify({ ok: true, pid: process.pid });
};

exports.onMessage = (f) => {
  cb = f;
};

exports.send = (text) => {
  if (text.length > 1000) return cb('big:' + text.length);
  // Blocks the child's loop, so the parent's IPC channel backs up.
  if (text === 'busy') {
    const until = Date.now() + 1500;
    while (Date.now() < until) {}
    return;
  }
  if (text === 'config') return cb(JSON.stringify(lastConfig));
  if (text === 'die') return global.mqHost.died('fake');
  if (text === 'reject') return void Promise.reject(new Error('boom'));
  if (text === 'stuckdie') {
    // Reports died but ignores SIGTERM and never exits: only SIGKILL ends it.
    process.on('SIGTERM', () => {});
    process.send({ kind: 'died', message: 'stuck' });
    for (;;) {}
  }
  if (text === 'pid') return cb(JSON.stringify({ pid: process.pid }));
  cb('echo:' + text);
};

const body = new Uint8Array(Buffer.from('WEBVTT\n'));
const answers = {
  '/sub/x.vtt': { status: 200, headers: [['Content-Type', 'text/vtt'], ['Cache-Control', 'no-store'], ['X-Content-Type-Options', 'sniff']], body },
  '/sub/status': { status: 99999, headers: [], body },
  '/sub/header': { status: 200, headers: [['Bad Header\n', 'x']], body },
};
exports.serve = (p) => Promise.resolve(answers[p] || null);

exports.fire = () => {};
exports.fetched = () => {};
exports.resolved = () => {};
exports.ping = () => '{"pong":true}';
